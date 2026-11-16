#!/bin/sh
# run-c-tests.sh - C-level tests of the SimdUTFPlugin engine and libSimdUTFFFI
#
# Builds tests/sutest.cpp with suEngine.cpp, suFFI.c (the library
# libSimdUTFFFI) and simdutf 9.4.0 (its unity file src/simdutf.cpp, with the
# six features of cmake/simdutf.cmake), and runs it (checks U1-U7 and U9, see
# sutest.cpp) in these lanes:
#
#   clang       clang++ -std=c++17 -O2 -fno-exceptions, clang -std=c99
#   fallback    the clang build under SIMDUTF_FORCE_IMPLEMENTATION=fallback
#               (the kernel of the web VMs, run natively)
#   guard       U8: one process of the clang build per value of
#               SIMDUTF_FORCE_IMPLEMENTATION (unset, fallback, haswell,
#               westmere, bogus, icelake, and 80 x, whose note is cut to
#               'refused ' and 55 x); a kernel the CPU lacks must be
#               refused, never run (no SIGILL)
#   gcc-asan    g++ -O2 -fsanitize=address,undefined
#   wasm64      em++ -m64 -O2, under node
#   memory64-2  em++ -sMEMORY64=2 -O2, under node (wasm32 with 64-bit
#               pointers lowered)
#   glue        U10, the glue SimdUTFPlugin.c against a fake VM (needs
#               --vm-build)
#
# and checks these static rules:
#
#   R1  the glue allocates only through instantiateClassindexableSize, in
#       allocateResult and primitiveInfo, and never with an eden-only
#       allocator nor malloc (signed64BitIntegerFor, positive64BitIntegerFor,
#       floatObjectOf, stringForCString, methodReturnString, clone)
#   R2  no simdutf header in the glue nor in suFFI.c
#   R3  no VM header in suEngine.cpp nor in suFFI.c
#   R4  no std::vector, std::string, new, malloc, calloc, realloc or throw
#       in the engine
#   R5  the primitives the glue exports, the rows of its builtin table and
#       its *AccessorDepth variables are the same set
#   R6  getenv only in choose() of suEngine.cpp, the guard of su_init
#   R7  src/common holds exactly one .c file (add_vm_plugin globs it)
#   V   the simdutf tree is 9.4.0
#
# Options:
#   --simdutf DIR  a simdutf 9.4.0 tree (src/simdutf.cpp, include/); default:
#                  the one the --vm-build tree fetched (deps/simdutf); required
#                  otherwise, except for --only R
#   --emsdk DIR    the Emscripten SDK; default: $EMSDK; without it the em++
#                  lanes are skipped
#   --node PATH    node for the em++ lanes; default: the newest node of the SDK
#   --vm-build DIR a configured native VM tree, for U10 (its include/pharovm)
#   --gen DIR      the GENERATED_SOURCE_DIR of that tree, for U10; default:
#                  read from DIR/CMakeCache.txt
#   --build DIR    where to build (kept, and reused by the next run);
#                  default: a temporary directory, removed at the end
#   --only LIST    comma-separated: checks (U1-U10), R for the rules, e.g.
#                  U1,U8,R; U8 is the guard lane, U10 the glue lane
#   SU_SLOW=1      adds round trips of 256 MiB inputs (sutest --slow)
#
# Nothing is written outside the build directory.  Exits 1 when a lane or
# a rule fails, 2 on a usage error.

set -e

SRCDIR=$(cd "$(dirname "$0")/.." && pwd)
simdutf= emsdk=${EMSDK:-} node= vmbuild= gen= build= only=
while test $# -gt 0; do
    case $1 in
    --simdutf) simdutf=$2; shift ;;
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

# The primitives of the contract (docs/SimdUTF.md, "The primitives")
glue_primitives="primitiveBase64Decode primitiveBase64Encode primitiveDecode primitiveEncode
 primitiveInfo primitiveMeasure primitiveValidate"

# R1, R5, R7: the rules of the glue
rules_glue() {
    glue=$SRCDIR/src/common/SimdUTFPlugin.c
    bad=0
    # R1: no eden-only allocator, no C heap
    if code_lines "$glue" | grep -E '(^|[^A-Za-z0-9_])(signed64BitIntegerFor|positive64BitIntegerFor|floatObjectOf|stringForCString|methodReturnString|clone|malloc|calloc|realloc)[ \t]*\('; then
        echo "R1: the glue calls an eden-only allocator or the C heap"
        bad=1
    fi
    # R1: instantiateClassindexableSize once in allocateResult (the results),
    # else only in primitiveInfo and its byteString
    sites=$(code_lines "$glue" | awk '
        { sub(/^[^:]*:[0-9]+: /, "") }
        /^[A-Za-z_][A-Za-z0-9_]*\(/ && !/;[ \t]*$/ { fn = $0; sub(/\(.*/, "", fn) }
        /^}/ { fn = "" }
        /instantiateClassindexableSize/ { n = gsub(/instantiateClassindexableSize/, "&"); for (i = 0; i < n; i++) print fn }')
    if test "$(echo "$sites" | grep -c '^allocateResult$')" != 1 \
        || echo "$sites" | grep -v -E '^(allocateResult|primitiveInfo|byteString)$' | grep -q .; then
        echo "R1: instantiateClassindexableSize is called in:" $sites
        bad=1
    fi
    # R5: the EXPORT(sqInt) primitives (declared and defined), the rows of
    # the builtin table (name and function alike) and the *AccessorDepth
    # variables are the same set, the seven primitives of the contract
    want=$(echo $glue_primitives | tr ' ' '\n' | sort)
    declared=$(sed -n 's/^EXPORT(sqInt) \(primitive[A-Za-z0-9]*\)(void);$/\1/p' "$glue" | sort)
    defined=$(awk '/^EXPORT\(sqInt\)$/ { getline; if (match($0, /^primitive[A-Za-z0-9]*\(void\)$/)) { sub(/\(.*/, ""); print } }' "$glue" | sort)
    rows=$(sed -n 's/^	{(void\*)_m, "\(primitive[A-Za-z0-9]*\)\\000\\000", (void\*)\(primitive[A-Za-z0-9]*\)},$/\1 \2/p' "$glue" \
        | awk '$1 == $2 { print $1 } $1 != $2 { print "mismatch:" $1 "/" $2 }' | sort)
    depths=$(sed -n 's/^EXPORT(signed char) \(primitive[A-Za-z0-9]*\)AccessorDepth = 0;$/\1/p' "$glue" | sort)
    for set in declared defined rows depths; do
        eval "value=\$$set"
        if test "$value" != "$want"; then
            echo "R5: the $set primitives are:" $value
            bad=1
        fi
    done
    # R7: add_vm_plugin compiles every .c of src/common
    commons=$(cd "$SRCDIR/src/common" && ls *.c 2>/dev/null) || true
    if test "$commons" != SimdUTFPlugin.c; then
        echo "R7: src/common holds:" $commons
        bad=1
    fi
    if test $bad = 0; then
        echo "R1 R5 R7 ok"
    else
        echo "R1 R5 R7 FAILED"
    fi
    return $bad
}

rules() {
    glue=$SRCDIR/src/common/SimdUTFPlugin.c
    engine=$SRCDIR/src/cxx/suEngine.cpp
    ffi=$SRCDIR/src/ffi/suFFI.c
    bad=0
    # R2
    for f in "$glue" "$ffi"; do
        if test -f "$f" && grep -n '#include.*simdutf' "$f"; then
            echo "R2: $f includes simdutf"
            bad=1
        fi
    done
    # R3
    for f in "$engine" "$ffi"; do
        if grep -n '#include *[<"]\(sq[A-Za-z]*\|virtualMachine\|interp\|pharovm/[A-Za-z]*\|vmCallback\|cointerp\)\.h[>"]' "$f"; then
            echo "R3: $f includes a VM header"
            bad=1
        fi
    done
    # R4
    if code_lines "$engine" | grep -E 'std::vector|std::string([^_]|$)|(^|[^A-Za-z0-9_])(new|malloc|calloc|realloc|throw)([^A-Za-z0-9_]|$)'; then
        echo "R4: the engine allocates or throws"
        bad=1
    fi
    # R6: one getenv, in choose()
    n=$(code_lines "$engine" "$ffi" $(test -f "$glue" && echo "$glue") | grep -c 'getenv' || true)
    inside=$(awk '/^choose\(\)/ { inside = 1 } inside && /getenv/ { n++ } /^}/ { inside = 0 } END { print n + 0 }' "$engine")
    if test "$n" != 1 || test "$inside" != 1; then
        echo "R6: getenv appears $n times, $inside of them in choose()"
        bad=1
    fi
    # V
    if test -n "$simdutf" && ! grep -q '^#define SIMDUTF_VERSION "9\.4\.0"' "$simdutf/include/simdutf/simdutf_version.h"; then
        echo "V: $simdutf is not simdutf 9.4.0"
        bad=1
    fi
    if test $bad = 0; then
        echo "R2 R3 R4 R6${simdutf:+ V} ok"
    else
        echo "R FAILED"
        failed=1
    fi
    rules_glue || failed=1
}

# the simdutf that the VM tree fetched, when --simdutf is not given
if test -z "$simdutf" && test -n "$vmbuild" && test -d "$vmbuild/deps/simdutf/simdutf-9.4.0"; then
    simdutf=$vmbuild/deps/simdutf/simdutf-9.4.0
fi
if test -n "$simdutf"; then
    simdutf=$(cd "$simdutf" && pwd)
    if ! test -f "$simdutf/src/simdutf.cpp" || ! test -f "$simdutf/include/simdutf/simdutf_version.h"; then
        echo "run-c-tests: $simdutf is not a simdutf source tree (no src/simdutf.cpp)" >&2
        exit 2
    fi
fi
if selected R; then
    rules
fi
# the checks of sutest that were asked; U8 and U10 are lanes of their own
tests= guard= glue=
for t in $(echo "$only" | tr ',' ' '); do
    case $t in
    R) ;;
    U8) guard=1 ;;
    U10) glue=1 ;;
    *) tests=${tests:+$tests,}$t ;;
    esac
done
if test -z "$only"; then
    guard=1
    test -n "$vmbuild" && glue=1
fi
if test -n "$only" && test -z "$tests" && test -z "$guard" && test -z "$glue"; then
    exit $failed
fi
if test -z "$simdutf"; then
    echo "run-c-tests: --simdutf DIR is required (a simdutf 9.4.0 tree)" >&2
    exit 2
fi
if test -n "$build"; then
    mkdir -p "$build"
    build=$(cd "$build" && pwd)
else
    build=$(mktemp -d "${TMPDIR:-/tmp}/sutest.XXXXXX")
    trap 'rm -rf "$build"' EXIT
fi
if test -n "$emsdk" && test -z "$node"; then
    node=$(ls -d "$emsdk"/node/*/bin/node 2>/dev/null | sort -V | tail -n 1)
fi
slow=
test "${SU_SLOW:-}" = 1 && slow=--slow

features="-DSIMDUTF_FEATURE_UTF8=1 -DSIMDUTF_FEATURE_UTF16=1 -DSIMDUTF_FEATURE_UTF32=1 -DSIMDUTF_FEATURE_ASCII=1
 -DSIMDUTF_FEATURE_LATIN1=1 -DSIMDUTF_FEATURE_BASE64=1 -DSIMDUTF_FEATURE_DETECT_ENCODING=0"
common="-std=c++17 -g0 -fno-exceptions $features -I$simdutf/include -I$SRCDIR/include/common -I$SRCDIR/include/ffi"

# lane_build NAME CXX CC "FLAGS" "LINK FLAGS" OUTPUT: builds $build/NAME/OUTPUT
lane_build() {
    name=$1 cxx=$2 cc=$3 flags=$4 link=$5
    dir=$build/$name
    mkdir -p "$dir"
    # simdutf takes the longest: built once per lane and kept
    if ! test -f "$dir/simdutf.o"; then
        $cxx $common -I"$simdutf/src" $flags -c "$simdutf/src/simdutf.cpp" -o "$dir/simdutf.o" || return 1
    fi
    $cc -std=c99 -g0 $flags -I"$SRCDIR/include/common" -I"$SRCDIR/include/ffi" -c "$SRCDIR/src/ffi/suFFI.c" -o "$dir/suFFI.o" &&
    $cxx $common $flags -c "$SRCDIR/src/cxx/suEngine.cpp" -o "$dir/suEngine.o" &&
    $cxx $common $flags -c "$SRCDIR/tests/sutest.cpp" -o "$dir/sutest.o" &&
    $cxx $flags $link -o "$dir/$6" "$dir/sutest.o" "$dir/suEngine.o" "$dir/suFFI.o" "$dir/simdutf.o"
}

lane_run() {
    name=$1 label=$2
    shift 2
    echo "== $label"
    if "$@" ${tests:+--only "$tests"} $slow; then
        echo "== $label: ok"
    else
        echo "== $label: FAILED"
        failed=1
    fi
}

em=
if test -n "$emsdk"; then
    em=$emsdk/upstream/emscripten
    if ! test -x "$em/em++" || ! test -x "$node"; then
        echo "run-c-tests: no em++ in $emsdk or no node ($node)" >&2
        exit 2
    fi
fi
# the setjmp/longjmp flags of the VM (PHARO_WASM_SJLJ_FLAGS, cmake/Emscripten.cmake)
emflags="-fno-exceptions -sSUPPORT_LONGJMP=wasm -sWASM_LEGACY_EXCEPTIONS=0"
emlink="-sNODERAWFS=1 -sALLOW_MEMORY_GROWTH=1 -sMAXIMUM_MEMORY=4GB -sSTACK_SIZE=8MB"

# U10: the glue against a fake VM.  The headers of the VM come from the
# tree of --vm-build: its config.h (build/include/pharovm), the generated
# interp.h of its GENERATED_SOURCE_DIR (--gen, else read from its
# CMakeCache.txt), and the include directories of the repository that the
# VM library exports.  Checks that the glue compiles without warnings as
# the shared library and as a builtin plugin, that the Emscripten table
# generator reads its exports, then builds tests/gluetest.c with fakevm.c,
# the glue, the engine (clang, ASan and UBSan) and the simdutf of the clang
# lane, once per mode, and runs both.
glue_lane() {
    echo "== glue (U10)"
    if test -z "$vmbuild" || ! test -f "$vmbuild/CMakeCache.txt"; then
        echo "== glue (U10): FAILED: --vm-build DIR, a configured VM tree, is required"
        return 1
    fi
    vmbuild=$(cd "$vmbuild" && pwd)
    if test -z "$gen"; then
        gen=$(sed -n 's/^GENERATED_SOURCE_DIR:[A-Z]*=//p' "$vmbuild/CMakeCache.txt")
    fi
    repo=$(cd "$SRCDIR/../.." && pwd)
    case $(uname -s) in
    Darwin) platform=osx ;;
    *) platform=unix ;;
    esac
    vminc="-I$SRCDIR/include/common -I$vmbuild/build/include/pharovm -I$gen/generated/64/vm/include
 -I$repo/include -I$repo/include/pharovm -I$repo/include/pharovm/common -I$repo/include/pharovm/$platform"
    for f in "$vmbuild/build/include/pharovm/config.h" "$gen/generated/64/vm/include/interp.h"; do
        if ! test -f "$f"; then
            echo "== glue (U10): FAILED: no $f (--vm-build, --gen)"
            return 1
        fi
    done
    # the definitions every VM source gets (CMakeLists.txt)
    vmdefs="-DLSB_FIRST=1 -DIMMUTABILITY=1 -DCOGMTVM=0 -DDEBUGVM=0 -DASYNC_FFI_QUEUE=1 -D_FILE_OFFSET_BITS=64"
    cc=${CC:-clang} cxx=${CXX:-clang++}
    dir=$build/glue
    mkdir -p "$dir"
    ok=1
    for mode in "" -DSQUEAK_BUILTIN_PLUGIN; do
        if $cc -std=gnu11 -fsyntax-only -Wall -Werror $vmdefs $mode $vminc "$SRCDIR/src/common/SimdUTFPlugin.c"; then
            echo "-- $cc -fsyntax-only -Wall -Werror ${mode:-(shared library)}: ok"
        else
            echo "-- $cc -fsyntax-only -Wall -Werror ${mode:-(shared library)}: FAILED"
            ok=0
        fi
    done
    if command -v cmake > /dev/null; then
        if cmake -DSOURCE="$SRCDIR/src/common/SimdUTFPlugin.c" -DTABLE=SimdUTFPlugin_exports \
                -DOUTPUT="$dir/SimdUTFPlugin_primitives.c" -P "$repo/cmake/emscripten/genPrimitiveTable.cmake" \
            && $cc -std=gnu11 -fsyntax-only -Wall -Werror $vmdefs -DSQUEAK_BUILTIN_PLUGIN $vminc \
                -I"$SRCDIR/src/common" "$dir/SimdUTFPlugin_primitives.c"; then
            echo "-- genPrimitiveTable.cmake: $(grep -c '^static void w_' "$dir/SimdUTFPlugin_primitives.c") trampolines, ok"
        else
            echo "-- genPrimitiveTable.cmake: FAILED"
            ok=0
        fi
    fi
    san="-fsanitize=address,undefined -fno-sanitize-recover=undefined -fno-omit-frame-pointer"
    cflags="-std=gnu11 -O1 -g $san $vmdefs $vminc"
    if ! $cxx $common -O1 -g $san -c "$SRCDIR/src/cxx/suEngine.cpp" -o "$dir/suEngine.o" > "$dir/build.log" 2>&1; then
        cat "$dir/build.log"
        echo "== glue (U10): FAILED (build)"
        return 1
    fi
    rounds=200000
    test -n "$slow" && rounds=2000000
    for mode in shared builtin; do
        defs=
        test $mode = builtin && defs="-DSQUEAK_BUILTIN_PLUGIN -DGLUE_BUILTIN"
        if ! { $cc $cflags $defs -c "$SRCDIR/src/common/SimdUTFPlugin.c" -o "$dir/glue-$mode.o" &&
            $cc $cflags $defs -c "$SRCDIR/tests/fakevm.c" -o "$dir/fakevm-$mode.o" &&
            $cc $cflags $defs -c "$SRCDIR/tests/gluetest.c" -o "$dir/gluetest-$mode.o" &&
            $cxx $san -o "$dir/gluetest-$mode" "$dir/gluetest-$mode.o" "$dir/glue-$mode.o" "$dir/fakevm-$mode.o" \
                "$dir/suEngine.o" "$build/clang/simdutf.o"; } > "$dir/build-$mode.log" 2>&1; then
            cat "$dir/build-$mode.log"
            echo "-- gluetest ($mode): FAILED (build)"
            ok=0
            continue
        fi
        echo "-- the glue as a $mode plugin:"
        if ! env -u SIMDUTF_FORCE_IMPLEMENTATION "$dir/gluetest-$mode" --rounds $rounds; then
            ok=0
        fi
    done
    if test $ok = 1; then
        echo "== glue (U10): ok"
        return 0
    fi
    echo "== glue (U10): FAILED"
    return 1
}

# the builds run in parallel, each in its own directory; with only U8 or
# U10 asked, only what they need
pids=
lanes="clang"
if test -n "$tests" || test -z "$only"; then
    lanes="clang gcc-asan${em:+ wasm64 memory64-2}"
fi
for lane in $lanes; do
    case $lane in
    clang) lane_build clang "${CXX:-clang++}" "${CC:-clang}" "-O2" "" sutest ;;
    gcc-asan) lane_build gcc-asan g++ gcc "-O2 -fsanitize=address,undefined -fno-sanitize-recover=undefined -fno-omit-frame-pointer" "" sutest ;;
    wasm64) lane_build wasm64 "$em/em++" "$em/emcc" "-O2 -m64 $emflags" "$emlink" sutest.js ;;
    memory64-2) lane_build memory64-2 "$em/em++" "$em/emcc" "-O2 -sMEMORY64=2 $emflags" "$emlink" sutest.js ;;
    esac > "$build/$lane.log" 2>&1 &
    pids="$pids $!"
done
builds=ok
for pid in $pids; do
    wait $pid || builds=failed
done
if test $builds = failed; then
    for lane in $lanes; do
        if test -s "$build/$lane.log"; then
            echo "== $build/$lane.log"
            tail -n 30 "$build/$lane.log"
        fi
    done
    echo "run-c-tests: a build failed"
    exit 1
fi

export ASAN_OPTIONS=${ASAN_OPTIONS:-detect_leaks=1} UBSAN_OPTIONS=${UBSAN_OPTIONS:-print_stacktrace=1}
if test -n "$tests" || test -z "$only"; then
    lane_run clang "clang++ -O2" env -u SIMDUTF_FORCE_IMPLEMENTATION "$build/clang/sutest"
    lane_run fallback "clang++ -O2, SIMDUTF_FORCE_IMPLEMENTATION=fallback" \
        env SIMDUTF_FORCE_IMPLEMENTATION=fallback "$build/clang/sutest"
    lane_run gcc-asan "g++ -O2 ASan+UBSan" env -u SIMDUTF_FORCE_IMPLEMENTATION "$build/gcc-asan/sutest"
    if test -n "$em"; then
        lane_run wasm64 "em++ -m64 (node)" "$node" "$build/wasm64/sutest.js"
        lane_run memory64-2 "em++ -sMEMORY64=2 (node)" "$node" "$build/memory64-2/sutest.js"
    else
        echo "== em++ lanes skipped: no --emsdk"
    fi
fi

# The guard: each value in its own process.  A value is honoured when it
# names a kernel that is compiled in and runtime-supported (sutest
# --kernels lists them), else refused; the best kernel runs then.
if test -n "$guard"; then
    echo "== guard (U8)"
    kernels=$(env -u SIMDUTF_FORCE_IMPLEMENTATION "$build/clang/sutest" --kernels) || { echo "sutest --kernels failed"; failed=1; }
    best=$(echo "$kernels" | sed -n 's/^best //p')
    supported=" $(echo "$kernels" | sed -n 's/^supported //p') "
    echo "best $best, runtime-supported:$supported"
    # on x86-64 Linux, icelake needs AVX-512 VBMI2: without it, it must be refused
    if test "$(uname -m)" = x86_64 && test -r /proc/cpuinfo && ! grep -q avx512_vbmi2 /proc/cpuinfo; then
        case $supported in
        *" icelake "*) echo "guard: icelake is listed as supported, but the CPU has no avx512_vbmi2"; failed=1 ;;
        esac
    fi
    guarded=ok
    for value in unset fallback haswell westmere bogus icelake; do
        if test $value = unset; then
            kernel=$best forced=
            set -- env -u SIMDUTF_FORCE_IMPLEMENTATION
        else
            case $supported in
            *" $value "*) kernel=$value forced=$value ;;
            *) kernel=$best forced="refused $value" ;;
            esac
            set -- env SIMDUTF_FORCE_IMPLEMENTATION=$value
        fi
        echo "-- SIMDUTF_FORCE_IMPLEMENTATION=$value: expecting $kernel, forced '$forced'"
        if ! "$@" "$build/clang/sutest" --only U8 --expect-kernel "$kernel" --expect-forced "$forced"; then
            echo "-- SIMDUTF_FORCE_IMPLEMENTATION=$value: FAILED (exit $?)"
            guarded=failed
        fi
    done
    # a value too long for su_info.forced (64 bytes, NUL included) is refused,
    # and its note cut to 'refused ' and the first 55 bytes of the value
    long=xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
    echo "-- SIMDUTF_FORCE_IMPLEMENTATION=<80 x>: expecting $best, forced 'refused <55 x>'"
    if ! env SIMDUTF_FORCE_IMPLEMENTATION=$long "$build/clang/sutest" --only U8 --expect-kernel "$best" \
        --expect-forced "refused $(echo $long | cut -c1-55)"; then
        echo "-- SIMDUTF_FORCE_IMPLEMENTATION=<80 x>: FAILED"
        guarded=failed
    fi
    if test $guarded = ok; then
        echo "== guard (U8): ok"
    else
        echo "== guard (U8): FAILED"
        failed=1
    fi
fi

if test -n "$glue"; then
    glue_lane || failed=1
fi

if test $failed = 0; then
    echo "run-c-tests: all lanes passed"
else
    echo "run-c-tests: FAILED"
fi
exit $failed
