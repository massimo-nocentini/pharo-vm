#!/bin/sh
# run-c-tests.sh - C-level tests of the TimSortPlugin: its engine, its
# resumable sort, its glue and libTimSortFFI
#
# One lane per test source; each lane compiles its test with the sources it
# needs (timsort.c, src/coro and the rest), as cmake/timsort.cmake and
# cmake/plugins.cmake compile them (-DNDEBUG and the renamed list_sort_impl
# for timsort.c, the commit as TS_TIMSORT_VERSION), and runs it:
#
#   engine         tests/enginetest.c, src/engine/tsEngine.c, timsort.c;
#                  clang -O2
#   engine-asan    the same, gcc -O1 -fsanitize=address,undefined
#   engine-wasm64  the same, emcc -m64 -O2, under node (needs --emsdk): the
#                  engine of the web VMs (no resumable sort there)
#   coro           tests/corotest.c, every src/coro/*.c, the engine,
#                  timsort.c; clang -O2 (only where tsCoro.h says
#                  TS_CORO_SUPPORTED)
#   coro-asan      the same, gcc -O1 -fsanitize=address,undefined
#   glue           tests/gluetest.c and tests/fakevm.c with the glue
#                  src/common/TimSortPlugin.c, the engine, src/coro and
#                  timsort.c, clang -O1 ASan and UBSan, once as the glue of a
#                  shared library and once as a builtin plugin's
#                  (-DSQUEAK_BUILTIN_PLUGIN -DGLUE_BUILTIN); first the glue
#                  must compile with -Wall -Werror in both modes against the
#                  VM's headers, and cmake/emscripten/genPrimitiveTable.cmake
#                  must read its exports table (needs --vm-build)
#   ffi            tests/ffitest.c, src/ffi/tsFFI.c, the engine, src/coro,
#                  timsort.c; clang -O2
#   ffi-asan       the same, gcc -O1 -fsanitize=address,undefined
#   ffi-exports    libTimSortFFI built as cmake/plugins.cmake builds it (a
#                  shared library, -fvisibility=hidden) exports exactly the
#                  TS_FFI_EXPORT functions of include/ffi/tsFFI.h (Linux)
#
# The sources of the plugin and of the library (not the tests, nor
# timsort.c) compile with -Wall -Wextra -Werror in the clang lanes.
# Each work package adds the flags its lane needs to ENGINE_FLAGS,
# CORO_FLAGS, GLUE_FLAGS or FFI_FLAGS below (with a note in ISSUES.md);
# the arguments of TS_TEST_ARGS (e.g. --slow) are given to every test.
#
# and checks these static rules (R):
#
#   R1  the glue allocates only through instantiateClassindexableSize, in
#       primitiveInfo and its byteString, and never with an eden-only
#       allocator nor the C heap (signed64BitIntegerFor,
#       positive64BitIntegerFor, floatObjectOf, stringForCString,
#       methodReturnString, clone, malloc, calloc, realloc)
#   R2  timsort.h is included by src/engine/tsEngine.c only
#   R3  no VM header in the engine, src/coro nor src/ffi
#   R5  the primitives the glue exports, the rows of its builtin table and
#       its *AccessorDepth variables are the same set, the eight primitives
#       of the contract, with the same depths: 1 for primitiveSortObjects
#       and primitiveArgSort, 0 for the others
#   R7  src/common holds exactly one .c file, TimSortPlugin.c (add_vm_plugin
#       globs it)
#   V   the timsort.c tree is the one of the pin (fd70381: the include
#       guard, the ptrdiff_t size type and list_sort_impl in timsort.h)
#
# Options:
#   --timsort DIR  a timsort.c tree at fd70381 (src/timsort.c, src/timsort.h);
#                  default: the one the --vm-build tree fetched
#                  (deps/timsort/timsort.c-<commit>); required otherwise,
#                  except for --only R
#   --emsdk DIR    the Emscripten SDK; default: $EMSDK; without it the lane
#                  engine-wasm64 is skipped
#   --node PATH    node for engine-wasm64; default: the newest node of the SDK
#   --vm-build DIR a configured native VM tree, for the lane glue (its
#                  include/pharovm)
#   --gen DIR      the GENERATED_SOURCE_DIR of that tree, for the lane glue;
#                  default: read from DIR/CMakeCache.txt
#   --build DIR    where to build (kept, and reused by the next run);
#                  default: a temporary directory, removed at the end
#   --only LIST    comma-separated lanes (above) and R, e.g. engine,glue,R;
#                  default: R and every lane that can run here (glue with
#                  --vm-build, engine-wasm64 with an SDK, coro and
#                  coro-asan where TS_CORO_SUPPORTED)
#
# Nothing is written outside the build directory.  Exits 1 when a lane or
# a rule fails, 2 on a usage error.

set -e

# the flags of each lane's sources and test, beyond those below (edited by
# the work package that owns the lane)
ENGINE_FLAGS=
CORO_FLAGS=
GLUE_FLAGS=
FFI_FLAGS=

TIMSORT_COMMIT=fd70381039da47507984b9c1306914828871afbf
TIMSORT_VERSION=fd70381

SRCDIR=$(cd "$(dirname "$0")/.." && pwd)
timsort= emsdk=${EMSDK:-} node= vmbuild= gen= build= only=
while test $# -gt 0; do
    case $1 in
    --timsort) timsort=$2; shift ;;
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

all_lanes="engine engine-asan engine-wasm64 coro coro-asan glue ffi ffi-asan ffi-exports"
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

# The primitives of the contract (docs/TimSort.md, "The primitives"), with
# their accessor depths
glue_primitives="primitiveArgSort:1 primitiveBlockSortFree:0 primitiveBlockSortNew:0
 primitiveBlockSortNext:0 primitiveBlockSortPermutation:0 primitiveInfo:0
 primitiveSortBuffer:0 primitiveSortObjects:1"

rules() {
    glue=$SRCDIR/src/common/TimSortPlugin.c
    engine=$SRCDIR/src/engine/tsEngine.c
    ffi=$SRCDIR/src/ffi/tsFFI.c
    coros=$(ls "$SRCDIR"/src/coro/*.c 2>/dev/null) || true
    bad=0
    # R1: no eden-only allocator, no C heap
    if code_lines "$glue" | grep -E '(^|[^A-Za-z0-9_])(signed64BitIntegerFor|positive64BitIntegerFor|floatObjectOf|stringForCString|methodReturnString|clone|malloc|calloc|realloc)[ \t]*\('; then
        echo "R1: the glue calls an eden-only allocator or the C heap"
        bad=1
    fi
    # R1: instantiateClassindexableSize only in primitiveInfo and byteString
    sites=$(code_lines "$glue" | awk '
        { sub(/^[^:]*:[0-9]+: /, "") }
        /^[A-Za-z_][A-Za-z0-9_]*\(/ && !/;[ \t]*$/ { fn = $0; sub(/\(.*/, "", fn) }
        /^}/ { fn = "" }
        /instantiateClassindexableSize/ { n = gsub(/instantiateClassindexableSize/, "&"); for (i = 0; i < n; i++) print (fn == "" ? "?" : fn) }')
    if echo "$sites" | grep -v -E '^(primitiveInfo|byteString)$' | grep -q .; then
        echo "R1: instantiateClassindexableSize is called in:" $sites
        bad=1
    fi
    # R2: timsort.h in the engine only
    for f in "$glue" "$ffi" $coros "$SRCDIR"/tests/*.c "$SRCDIR"/tests/*.h "$SRCDIR"/include/*/*.h; do
        if test -f "$f" && grep -n '#include *[<"]timsort\.[ch][>"]' "$f"; then
            echo "R2: $f includes timsort.h"
            bad=1
        fi
    done
    if ! grep -q '#include *"timsort\.h"' "$engine"; then
        echo "R2: $engine does not include timsort.h"
        bad=1
    fi
    # R3: no VM header outside the glue (and its tests)
    for f in "$engine" "$ffi" $coros "$SRCDIR"/include/common/*.h "$SRCDIR"/include/ffi/*.h; do
        if grep -n '#include *[<"]\(sq[A-Za-z]*\|virtualMachine\|interp\|pharovm/[A-Za-z]*\|vmCallback\|cointerp\)\.h[>"]' "$f"; then
            echo "R3: $f includes a VM header"
            bad=1
        fi
    done
    # R5: the EXPORT(sqInt) primitives (declared and defined), the rows of
    # the builtin table (name and function alike, with the depth) and the
    # *AccessorDepth variables (with the depth) are the same set
    want=$(echo $glue_primitives | tr ' ' '\n' | sort)
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
    if test "$commons" != TimSortPlugin.c; then
        echo "R7: src/common holds:" $commons
        bad=1
    fi
    # V: the header of the pinned commit (cmake/timsort.cmake checks the same)
    if test -n "$timsort"; then
        marks=$(grep -c -E '^(#ifndef TIMSORT_H|#define TIMSORT_SSIZE_T_MAX PTRDIFF_MAX|int list_sort_impl\()' "$timsort/src/timsort.h" || true)
        if test "$marks" != 3; then
            echo "V: $timsort is not timsort.c $TIMSORT_VERSION"
            bad=1
        fi
    fi
    if test $bad = 0; then
        echo "R1 R2 R3 R5 R7${timsort:+ V} ok"
    else
        echo "R FAILED"
        failed=1
    fi
}

# the timsort.c that the VM tree fetched, when --timsort is not given
if test -z "$timsort" && test -n "$vmbuild" && test -d "$vmbuild/deps/timsort/timsort.c-$TIMSORT_COMMIT"; then
    timsort=$vmbuild/deps/timsort/timsort.c-$TIMSORT_COMMIT
fi
if test -n "$timsort"; then
    timsort=$(cd "$timsort" && pwd)
    if ! test -f "$timsort/src/timsort.c" || ! test -f "$timsort/src/timsort.h"; then
        echo "run-c-tests: $timsort is not a timsort.c source tree (no src/timsort.c)" >&2
        exit 2
    fi
fi
if selected R; then
    rules
fi

# the lanes to run
cc=${CC:-clang}
if test -n "$emsdk" && test -z "$node"; then
    node=$(ls -d "$emsdk"/node/*/bin/node 2>/dev/null | sort -V | tail -n 1)
fi
coro_supported=0
if printf '#include "tsCoro.h"\n#if !TS_CORO_SUPPORTED\n#error no switch\n#endif\n' \
        | $cc -E -I"$SRCDIR/include/common" -x c - > /dev/null 2>&1; then
    coro_supported=1
fi
lanes=
for lane in $all_lanes; do
    if test -n "$only"; then
        selected $lane || continue
    else
        case $lane in
        engine-wasm64) test -n "$emsdk" || { echo "== engine-wasm64 skipped: no --emsdk"; continue; } ;;
        coro|coro-asan) test $coro_supported = 1 || { echo "== $lane skipped: TS_CORO_SUPPORTED is 0 here"; continue; } ;;
        glue) test -n "$vmbuild" || { echo "== glue skipped: no --vm-build"; continue; } ;;
        ffi-exports) test "$(uname -s)" = Linux || { echo "== ffi-exports skipped: not Linux"; continue; } ;;
        esac
    fi
    lanes="$lanes $lane"
done
if test -z "$lanes"; then
    test $failed = 0 && echo "run-c-tests: all lanes passed" || echo "run-c-tests: FAILED"
    exit $failed
fi
if test -z "$timsort"; then
    echo "run-c-tests: --timsort DIR is required (a timsort.c tree at $TIMSORT_VERSION)" >&2
    exit 2
fi
if test -n "$build"; then
    mkdir -p "$build"
    build=$(cd "$build" && pwd)
else
    build=$(mktemp -d "${TMPDIR:-/tmp}/tstest.XXXXXX")
    trap 'rm -rf "$build"' EXIT
fi
em=
if test -n "$emsdk"; then
    em=$emsdk/upstream/emscripten
fi

coro_sources=$(ls "$SRCDIR"/src/coro/*.c 2>/dev/null) || true
# what every source gets (cmake/timsort.cmake, cmake/plugins.cmake)
defs="-DTS_TIMSORT_VERSION=\"$TIMSORT_VERSION\" -Dlist_sort_impl=pharo_timsort_list_sort_impl"
incs="-I$SRCDIR/include/common -I$SRCDIR/include/ffi -I$timsort/src"
strict="-Wall -Wextra -Werror"
asan="-fsanitize=address,undefined -fno-sanitize-recover=undefined -fno-omit-frame-pointer"

# compile DIR CC "FLAGS" SOURCE...: each source into DIR/<name>.o; the
# objects in $objects
compile() {
    cdir=$1 ccompiler=$2 cflags=$3
    shift 3
    objects=
    for source in "$@"; do
        object=$cdir/$(basename "$source" .c).o
        $ccompiler -std=gnu11 $cflags -c "$source" -o "$object" || return 1
        objects="$objects $object"
    done
}

# lane_build NAME CC "OPT" "STRICT" "LINK" OUTPUT "LANE FLAGS" TEST SOURCE...:
# timsort.c with OPT, the sources with OPT and STRICT, the test with OPT,
# linked into $build/NAME/OUTPUT
lane_build() {
    name=$1 ccc=$2 opt=$3 strictness=$4 link=$5 output=$6 laneflags=$7 test=$8
    shift 8
    dir=$build/$name
    mkdir -p "$dir"
    compile "$dir" "$ccc" "$opt -DNDEBUG $defs $incs" "$timsort/src/timsort.c" || return 1
    all=$objects
    compile "$dir" "$ccc" "$opt $strictness $defs $incs $laneflags" "$@" || return 1
    all="$all $objects"
    compile "$dir" "$ccc" "$opt -Wall $defs $incs $laneflags" "$test" || return 1
    $ccc $opt $link -o "$dir/$output" $all $objects
}

# lane_run NAME COMMAND...: runs a lane's test
lane_run() {
    name=$1
    shift
    echo "== $name"
    if "$@" ${TS_TEST_ARGS:-}; then
        echo "== $name: ok"
    else
        echo "== $name: FAILED"
        failed=1
    fi
}

# lane NAME: builds and runs it
lane() {
    log=$build/$1.log
    case $1 in
    engine) set -- lane_build engine "$cc" -O2 "$strict" "" enginetest "$ENGINE_FLAGS" \
                "$SRCDIR/tests/enginetest.c" "$SRCDIR/src/engine/tsEngine.c" ;;
    engine-asan) set -- lane_build engine-asan gcc "-O1 -g $asan" "" "" enginetest "$ENGINE_FLAGS" \
                "$SRCDIR/tests/enginetest.c" "$SRCDIR/src/engine/tsEngine.c" ;;
    engine-wasm64) set -- lane_build engine-wasm64 "$em/emcc" "-O2 -m64" "$strict" \
                "-sNODERAWFS=1 -sALLOW_MEMORY_GROWTH=1 -sMAXIMUM_MEMORY=16GB -sSTACK_SIZE=8MB" \
                enginetest.js "$ENGINE_FLAGS" "$SRCDIR/tests/enginetest.c" "$SRCDIR/src/engine/tsEngine.c" ;;
    coro) set -- lane_build coro "$cc" -O2 "$strict" "" corotest "$CORO_FLAGS" \
                "$SRCDIR/tests/corotest.c" "$SRCDIR/src/engine/tsEngine.c" $coro_sources ;;
    coro-asan) set -- lane_build coro-asan gcc "-O1 -g $asan" "" "" corotest "$CORO_FLAGS" \
                "$SRCDIR/tests/corotest.c" "$SRCDIR/src/engine/tsEngine.c" $coro_sources ;;
    ffi) set -- lane_build ffi "$cc" -O2 "$strict" "" ffitest "$FFI_FLAGS" \
                "$SRCDIR/tests/ffitest.c" "$SRCDIR/src/ffi/tsFFI.c" "$SRCDIR/src/engine/tsEngine.c" $coro_sources ;;
    ffi-asan) set -- lane_build ffi-asan gcc "-O1 -g $asan" "" "" ffitest "$FFI_FLAGS" \
                "$SRCDIR/tests/ffitest.c" "$SRCDIR/src/ffi/tsFFI.c" "$SRCDIR/src/engine/tsEngine.c" $coro_sources ;;
    esac
    if ! "$@" > "$log" 2>&1; then
        cat "$log"
        echo "== $name: FAILED (build)"
        failed=1
        return 0
    fi
    case $name in
    engine-wasm64)
        if ! test -x "$node"; then
            echo "== engine-wasm64: FAILED: no node ($node)"
            failed=1
            return 0
        fi
        lane_run "$name" "$node" "$build/$name/enginetest.js" ;;
    *) lane_run "$name" "$build/$name/$7" ;;
    esac
}

# libTimSortFFI as cmake/plugins.cmake builds it: its dynamic symbols are
# the TS_FFI_EXPORT functions of tsFFI.h, nothing more, nothing less
ffi_exports() {
    echo "== ffi-exports"
    dir=$build/ffi-exports
    mkdir -p "$dir"
    if ! $cc -std=gnu11 -O2 -fPIC -fvisibility=hidden -DNDEBUG $defs $incs -c "$timsort/src/timsort.c" -o "$dir/timsort.o" > "$dir/build.log" 2>&1 \
        || ! $cc -std=gnu11 -O2 -shared -fPIC -fvisibility=hidden $strict $defs $incs $FFI_FLAGS -o "$dir/libTimSortFFI.so" \
            "$SRCDIR/src/ffi/tsFFI.c" "$SRCDIR/src/engine/tsEngine.c" $coro_sources "$dir/timsort.o" -Wl,--no-undefined >> "$dir/build.log" 2>&1; then
        cat "$dir/build.log"
        echo "== ffi-exports: FAILED (build)"
        failed=1
        return 0
    fi
    want=$(sed -n 's/^TS_FFI_EXPORT [^(]*[ *]\(ts_ffi_[a-z0-9_]*\)(.*/\1/p' "$SRCDIR/include/ffi/tsFFI.h" | sort)
    have=$(nm -D --defined-only "$dir/libTimSortFFI.so" | awk '$2 ~ /^[TDBRVW]$/ { print $3 }' | sed 's/@.*//' | sort)
    if test "$want" = "$have"; then
        echo "exports: $(echo $have | wc -w) functions, the TS_FFI_EXPORT functions of tsFFI.h"
        echo "== ffi-exports: ok"
    else
        echo "tsFFI.h:" $want
        echo "libTimSortFFI.so:" $have
        echo "== ffi-exports: FAILED"
        failed=1
    fi
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
    dir=$build/glue
    mkdir -p "$dir"
    ok=1
    for mode in "" -DSQUEAK_BUILTIN_PLUGIN; do
        if $cc -std=gnu11 -fsyntax-only -Wall -Werror $vmdefs $mode $defs $incs $vminc $GLUE_FLAGS "$SRCDIR/src/common/TimSortPlugin.c"; then
            echo "-- $cc -fsyntax-only -Wall -Werror ${mode:-(shared library)}: ok"
        else
            echo "-- $cc -fsyntax-only -Wall -Werror ${mode:-(shared library)}: FAILED"
            ok=0
        fi
    done
    if command -v cmake > /dev/null; then
        if cmake -DSOURCE="$SRCDIR/src/common/TimSortPlugin.c" -DTABLE=TimSortPlugin_exports \
                -DOUTPUT="$dir/TimSortPlugin_primitives.c" -P "$repo/cmake/emscripten/genPrimitiveTable.cmake" \
            && $cc -std=gnu11 -fsyntax-only -Wall -Werror $vmdefs -DSQUEAK_BUILTIN_PLUGIN $defs $incs $vminc $GLUE_FLAGS \
                -I"$SRCDIR/src/common" "$dir/TimSortPlugin_primitives.c"; then
            echo "-- genPrimitiveTable.cmake: $(grep -c '^static void w_' "$dir/TimSortPlugin_primitives.c") trampolines, ok"
        else
            echo "-- genPrimitiveTable.cmake: FAILED"
            ok=0
        fi
    fi
    flags="-std=gnu11 -O1 -g $asan $defs $incs"
    for mode in shared builtin; do
        modedefs=
        test $mode = builtin && modedefs="-DSQUEAK_BUILTIN_PLUGIN -DGLUE_BUILTIN"
        out=$dir/$mode
        mkdir -p "$out"
        if ! { compile "$out" "$cc" "$flags -DNDEBUG" "$timsort/src/timsort.c" && all=$objects &&
            compile "$out" "$cc" "$flags $GLUE_FLAGS" "$SRCDIR/src/engine/tsEngine.c" $coro_sources && all="$all $objects" &&
            compile "$out" "$cc" "$flags $vmdefs $vminc $modedefs $GLUE_FLAGS" \
                "$SRCDIR/src/common/TimSortPlugin.c" "$SRCDIR/tests/fakevm.c" "$SRCDIR/tests/gluetest.c" &&
            $cc $asan -o "$out/gluetest" $all $objects; } > "$out/build.log" 2>&1; then
            cat "$out/build.log"
            echo "-- gluetest ($mode): FAILED (build)"
            ok=0
            continue
        fi
        echo "-- the glue as a $mode plugin:"
        if ! "$out/gluetest" ${TS_TEST_ARGS:-}; then
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

export ASAN_OPTIONS=${ASAN_OPTIONS:-detect_leaks=1} UBSAN_OPTIONS=${UBSAN_OPTIONS:-print_stacktrace=1}
for name in $lanes; do
    case $name in
    glue) glue_lane || failed=1 ;;
    ffi-exports) ffi_exports ;;
    engine-wasm64)
        if test -z "$em" || ! test -x "$em/emcc"; then
            echo "== engine-wasm64: FAILED: no emcc (--emsdk)"
            failed=1
        else
            lane $name
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
