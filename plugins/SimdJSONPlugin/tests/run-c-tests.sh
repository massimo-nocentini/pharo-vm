#!/bin/sh
# run-c-tests.sh - C-level tests of the SimdJSONPlugin engine
#
# Builds tests/sjtest.cpp with sjEngine.cpp, sjText.c and the simdjson
# amalgamation, and runs it (checks T1-T8, see sjtest.cpp) with
#
#   clang++ -O2
#   g++ -O2 -fsanitize=address,undefined
#   em++ -m64 -O2, under node (wasm64)
#   em++ -sMEMORY64=2 -O2, under node (wasm32 with 64-bit pointers lowered)
#
# The two em++ lanes need --emsdk.  T9 is a static check made by this
# script: no eden-only allocator in SimdJSONPlugin.c (signed64BitIntegerFor,
# positive64BitIntegerFor, stringForCString, or floatObjectOf other than
# right after sj_is_small_float), no std::vector, std::string or throwing
# new in the engine, and no VM header in the engine nor simdjson.h in the
# glue.
#
# Options:
#   --simdjson DIR  a simdjson 5.0.5 tree (with singleheader/), or a directory
#                   holding simdjson.cpp and simdjson.h; required except for
#                   --only T9
#   --corpus DIR    the simdjson-data checkout, or its jsonexamples directory;
#                   default: $SJ_CORPUS; without it the corpus checks are skipped
#   --emsdk DIR     the Emscripten SDK; default: $EMSDK
#   --node PATH     node for the em++ lanes; default: the newest node of the SDK
#   --build DIR     where to build (kept, and reused by the next run);
#                   default: a temporary directory, removed at the end
#   --only LIST     comma-separated checks, e.g. T1,T7 or T9
#   --report        only T10: costs of parse, gate, enumeration, measure and
#                   walk on six corpus files (clang++ -O3)
#   SJ_SLOW=1       adds the 16,777,216-element array to T8
#
# Nothing is written outside the build directory.  Exits 1 when a lane or
# T9 fails, 2 on a usage error.

set -e

SRCDIR=$(cd "$(dirname "$0")/.." && pwd)
simdjson= corpus=${SJ_CORPUS:-} emsdk=${EMSDK:-} node= build= only= report=
while test $# -gt 0; do
    case $1 in
    --simdjson) simdjson=$2; shift ;;
    --corpus) corpus=$2; shift ;;
    --emsdk) emsdk=$2; shift ;;
    --node) node=$2; shift ;;
    --build) build=$2; shift ;;
    --only) only=$2; shift ;;
    --report) report=1 ;;
    *) echo "run-c-tests: unknown option $1" >&2; exit 2 ;;
    esac
    shift
done

selected() {
    test -z "$only" || case ",$only," in *",$1,"*) true ;; *) false ;; esac
}

failed=0

# T9
t9() {
    glue=$SRCDIR/src/common/SimdJSONPlugin.c
    engine="$SRCDIR/src/cxx/sjEngine.cpp $SRCDIR/src/common/sjText.c"
    bad=0
    if test -f "$glue"; then
        if grep -n 'signed64BitIntegerFor\|positive64BitIntegerFor\|stringForCString' "$glue"; then
            bad=1
        fi
        # floatObjectOf( only on the line of sj_is_small_float or the next one
        if awk '/floatObjectOf\(/ && !/^[ \t]*(extern|#)/ && !/sj_is_small_float/ && prev !~ /sj_is_small_float/ {
                    print FILENAME ":" NR ": " $0; found = 1 }
                { prev = $0 } END { exit !found }' "$glue"; then
            bad=1
        fi
        if grep -n '#include.*simdjson\.h' "$glue"; then
            bad=1
        fi
    else
        echo "T9: no $glue: the rules of the glue cannot be checked"
        bad=1
    fi
    # comments removed, then std::vector, std::string (not std::string_view)
    # and new without nothrow
    for f in $engine; do
        if awk '{ line = $0
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
                  if (line ~ /std::vector|std::string([^_]|$)/ || (line ~ /(^|[^A-Za-z0-9_])new([^A-Za-z0-9_]|$)/ && line !~ /nothrow/)) {
                      print FILENAME ":" NR ": " $0; found = 1 }
                }
                END { exit !found }' "$f"; then
            bad=1
        fi
        if grep -n '#include *"\(sq[A-Za-z]*\|virtualMachine\|interp\|pharovm/[a-z]*\)\.h"' "$f"; then
            bad=1
        fi
    done
    if test $bad = 0; then
        echo "T9 ok"
    else
        echo "T9 FAILED"
        failed=1
    fi
}

if test -z "$report" && selected T9; then
    t9
fi
case ",$only," in
    ,T9,) exit $failed ;;
esac
tests=
for t in $(echo "$only" | tr ',' ' '); do
    test "$t" = T9 || tests=${tests:+$tests,}$t
done

if test -z "$simdjson"; then
    echo "run-c-tests: --simdjson DIR is required (a simdjson 5.0.5 tree)" >&2
    exit 2
fi
simdjson=$(cd "$simdjson" && pwd)
test -f "$simdjson/singleheader/simdjson.cpp" && simdjson=$simdjson/singleheader
if ! test -f "$simdjson/simdjson.cpp" || ! grep -q '^#define SIMDJSON_VERSION "5\.0\.5"' "$simdjson/simdjson.h"; then
    echo "run-c-tests: $simdjson holds no simdjson 5.0.5 amalgamation" >&2
    exit 2
fi
if test -n "$corpus"; then
    corpus=$(cd "$corpus" && pwd)
    test -d "$corpus/jsonexamples" && corpus=$corpus/jsonexamples
fi
if test -n "$build"; then
    mkdir -p "$build"
    build=$(cd "$build" && pwd)
else
    build=$(mktemp -d "${TMPDIR:-/tmp}/sjtest.XXXXXX")
    trap 'rm -rf "$build"' EXIT
fi
if test -n "$emsdk" && test -z "$node"; then
    node=$(ls -d "$emsdk"/node/*/bin/node 2>/dev/null | sort -V | tail -n 1)
fi

common="-std=c++17 -g0 -fno-exceptions -DSIMDJSON_EXCEPTIONS=0 -I$simdjson -I$SRCDIR/include/common"

# lane NAME CXX CC "FLAGS" "LINK FLAGS" RUNNER: builds $build/NAME/sjtest[.js]
lane_build() {
    name=$1 cxx=$2 cc=$3 flags=$4 link=$5
    dir=$build/$name
    mkdir -p "$dir"
    # the amalgamation takes the longest: built once per lane and kept
    if ! test -f "$dir/simdjson.o"; then
        $cxx $common $flags -c "$simdjson/simdjson.cpp" -o "$dir/simdjson.o" || return 1
    fi
    $cc -std=c99 -g0 $flags -I"$SRCDIR/include/common" -c "$SRCDIR/src/common/sjText.c" -o "$dir/sjText.o" &&
    $cxx $common $flags -DSJ_TEST_HOOKS -c "$SRCDIR/src/cxx/sjEngine.cpp" -o "$dir/sjEngine.o" &&
    $cxx $common $flags -c "$SRCDIR/tests/sjtest.cpp" -o "$dir/sjtest.o" &&
    $cxx $flags $link -o "$dir/$6" "$dir/sjtest.o" "$dir/sjEngine.o" "$dir/sjText.o" "$dir/simdjson.o"
}

lane_run() {
    name=$1 label=$2
    shift 2
    echo "== $label"
    if "$@" ${corpus:+--corpus "$corpus"} ${tests:+--only "$tests"}; then
        echo "== $label: ok"
    else
        echo "== $label: FAILED"
        failed=1
    fi
}

if test -n "$report"; then
    lane_build report clang++ clang "-O3" "" sjtest || exit 1
    "$build/report/sjtest" ${corpus:+--corpus "$corpus"} --report
    exit 0
fi

em=
if test -n "$emsdk"; then
    em=$emsdk/upstream/emscripten
    if ! test -x "$em/em++" || ! test -x "$node"; then
        echo "run-c-tests: no em++ in $emsdk or no node ($node)" >&2
        exit 2
    fi
fi
emflags="-sNODERAWFS=1 -sALLOW_MEMORY_GROWTH=1 -sMAXIMUM_MEMORY=4GB -sSTACK_SIZE=8MB"

# the four builds run in parallel, each in its own directory
pids=
lane_build clang "${CXX:-clang++}" "${CC:-clang}" "-O2" "" sjtest > "$build/clang.log" 2>&1 & pids="$pids $!"
lane_build gcc-asan g++ gcc "-O2 -fsanitize=address,undefined -fno-sanitize-recover=undefined -fno-omit-frame-pointer" "" sjtest \
    > "$build/gcc-asan.log" 2>&1 & pids="$pids $!"
if test -n "$em"; then
    lane_build wasm64 "$em/em++" "$em/emcc" "-O2 -m64" "$emflags" sjtest.js > "$build/wasm64.log" 2>&1 & pids="$pids $!"
    lane_build memory64-2 "$em/em++" "$em/emcc" "-O2 -sMEMORY64=2" "$emflags" sjtest.js > "$build/memory64-2.log" 2>&1 & pids="$pids $!"
fi
builds=ok
for pid in $pids; do
    wait $pid || builds=failed
done
if test $builds = failed; then
    for log in "$build"/*.log; do
        if test -s "$log"; then
            echo "== $log"
            tail -n 30 "$log"
        fi
    done
    echo "run-c-tests: a build failed"
    exit 1
fi

export ASAN_OPTIONS=${ASAN_OPTIONS:-detect_leaks=1} UBSAN_OPTIONS=${UBSAN_OPTIONS:-print_stacktrace=1}
lane_run clang "clang++ -O2" "$build/clang/sjtest"
lane_run gcc-asan "g++ -O2 ASan+UBSan" "$build/gcc-asan/sjtest"
if test -n "$em"; then
    lane_run wasm64 "em++ -m64 (node)" "$node" "$build/wasm64/sjtest.js"
    lane_run memory64-2 "em++ -sMEMORY64=2 (node)" "$node" "$build/memory64-2/sjtest.js"
else
    echo "== em++ lanes skipped: no --emsdk"
fi
if test $failed = 0; then
    echo "run-c-tests: all lanes passed"
else
    echo "run-c-tests: FAILED"
fi
exit $failed
