#!/bin/sh
# glue.sh - the lane glue of run-c-tests.sh, which calls it: the tests of the
# SimdCSVPlugin's glue (src/common/SimdCSVPlugin.c) against a fake VM
#
#   1. the static rules of the glue:
#      R1  it calls only the proxy functions of the list below, and neither
#          the C heap nor any other function that could collect garbage
#      R5  its four primitives, declared and defined, the rows of its builtin
#          table (8, of which 4 primitives) and its *AccessorDepth variables
#          are the same, all of depth 0
#      R7  src/common holds exactly one .c file, SimdCSVPlugin.c, and it
#          includes no file but the VM's, scEngine.h, stdint.h and string.h
#   2. it compiles with -Wall -Werror against the VM's headers, as C99 (GNU)
#      and as C11, for a shared library and as a builtin plugin, each also
#      after the names that windows.h defines (windowsNames.h), and
#      cmake/emscripten/genPrimitiveTable.cmake reads its table
#   3. tests/gluetest.c and tests/fakevm.c with the glue and the engine, clang
#      -O1 -g, AddressSanitizer and UndefinedBehaviorSanitizer, once as the
#      glue of a shared library and once as a builtin plugin's
#      (-DSQUEAK_BUILTIN_PLUGIN -DGLUE_BUILTIN), each with the best kernel of
#      the machine and with the fallback
#
# Options:
#   --vm-build DIR a configured native VM tree (its build/include/pharovm and,
#                  with the plugin on, the slice of simdcsv); required
#   --gen DIR      the GENERATED_SOURCE_DIR of that tree; default: read from
#                  its CMakeCache.txt, else the tree
#   --slice DIR    the directory of simdcsv_slice.inc; default:
#                  deps/simdcsv-slice of the tree of --vm-build
#   --build DIR    where to build (kept); default: a temporary directory,
#                  removed at the end
#   --rounds N     the random calls of gluetest; default 200000
#
# CC and CXX: the compilers, default clang and clang++.  Nothing is written
# outside of the build directory.  Exits 1 when a check fails, 2 on a usage
# error.

set -e

SRCDIR=$(cd "$(dirname "$0")/.." && pwd)
repo=$(cd "$SRCDIR/../.." && pwd)
cc=${CC:-clang} cxx=${CXX:-clang++}
vmbuild= gen= slice= build= rounds=200000
while test $# -gt 0; do
    case $1 in
    --vm-build) vmbuild=$2; shift ;;
    --gen) gen=$2; shift ;;
    --slice) slice=$2; shift ;;
    --build) build=$2; shift ;;
    --rounds) rounds=$2; shift ;;
    *) echo "glue: unknown option $1" >&2; exit 2 ;;
    esac
    shift
done
if test -z "$vmbuild" || ! test -f "$vmbuild/CMakeCache.txt"; then
    echo "glue: --vm-build DIR, a configured VM tree, is required" >&2
    exit 2
fi
vmbuild=$(cd "$vmbuild" && pwd)
if test -z "$gen"; then
    gen=$(sed -n 's/^GENERATED_SOURCE_DIR:[A-Z]*=//p' "$vmbuild/CMakeCache.txt")
    test -n "$gen" || gen=$vmbuild
fi
test -n "$slice" || slice=$vmbuild/deps/simdcsv-slice
for f in "$vmbuild/build/include/pharovm/config.h" "$gen/generated/64/vm/include/interp.h" "$slice/simdcsv_slice.inc"; do
    if ! test -f "$f"; then
        echo "glue: no $f (--vm-build, --gen, --slice)" >&2
        exit 2
    fi
done
if test -z "$build"; then
    build=$(mktemp -d "${TMPDIR:-/tmp}/simdcsv-glue.XXXXXX")
    trap 'rm -rf "$build"' EXIT
fi
mkdir -p "$build"
build=$(cd "$build" && pwd)

glue=$SRCDIR/src/common/SimdCSVPlugin.c
bad=0
failed() {
    echo "-- $1: FAILED"
    bad=1
}

echo "== glue"

# The code of the files: comments and preprocessor lines removed, and the
# contents of the string literals
code() {
    awk '{ line = $0
           while (1) {
               if (incomment) { e = index(line, "*/"); if (!e) { line = ""; break }
                                line = substr(line, e + 2); incomment = 0 }
               s = index(line, "/*")
               if (!s) break
               rest = substr(line, s + 2); line = substr(line, 1, s - 1); incomment = 1
               e = index(rest, "*/"); if (e) { line = line " " substr(rest, e + 2); incomment = 0 } else break
           }
           if (line ~ /^[ \t]*#/) next
           print FNR ": " line }' "$1" | sed 's/"\([^"\\]\|\\.\)*"/""/g'
}

# R1
allowed="instantiateClassindexableSize integerObjectOf floatObjectOf storePointerofObjectwithValue
 firstIndexableField methodReturnValue primitiveFailFor primitiveFailureCode stackValue methodArgumentCount
 fetchClassOf fetchPointerofObject isBytes isWords isArray isPointers isIntegerObject isOopImmutable
 isYoung integerValueOf slotSizeOf byteSizeOf classArray classByteArray classFloat classString nilObject
 majorVersion minorVersion"
rule=ok
for f in $(code "$glue" | grep -o 'interpreterProxy->[A-Za-z0-9_]*' | sed 's/.*->//' | sort -u); do
    case " $(echo $allowed) " in
    *" $f "*) ;;
    *) echo "R1: the glue calls interpreterProxy->$f"; rule=FAILED ;;
    esac
done
if code "$glue" | grep -E '(^|[^A-Za-z0-9_])(signed64BitIntegerFor|positive64BitIntegerFor|stringForCString|methodReturnString|clone|pushRemappableOop|malloc|calloc|realloc|free|alloca|getenv|printf|strtod)[ 	]*\('; then
    echo "R1: the glue calls a forbidden function"
    rule=FAILED
fi
# floatObjectOf of a SmallFloat64 only, the one that allocates nothing
if test "$(code "$glue" | grep -c 'floatObjectOf')" != 1 \
 || ! code "$glue" | grep -B1 'floatObjectOf' | grep -q 'isSmallFloat(number)'; then
    echo "R1: floatObjectOf is not called once, after isSmallFloat"
    rule=FAILED
fi
echo "-- R1 (proxy functions): $rule"
test $rule = ok || bad=1

# R5
rule=ok
want=$(printf '%s:0\n' primitiveIndex primitiveInfo primitiveMeasure primitiveRows)
names=$(echo "$want" | sed 's/:.*//')
declared=$(sed -n 's/^EXPORT(sqInt) \(primitive[A-Za-z0-9]*\)(void);$/\1/p' "$glue" | sort)
defined=$(awk '/^EXPORT\(sqInt\)$/ { getline; if (match($0, /^primitive[A-Za-z0-9]*\(void\)$/)) { sub(/\(.*/, ""); print } }' "$glue" | sort)
rows=$(sed -n 's/^	{(void\*)_m, "\(primitive[A-Za-z0-9]*\)\\000\\00\([0-7]\)", (void\*)\(primitive[A-Za-z0-9]*\)},$/\1:\2 \3/p' "$glue" \
    | awk '{ split($1, a, ":"); print (a[1] == $2 ? $1 : "mismatch:" $1) }' | sort)
depths=$(sed -n 's/^EXPORT(signed char) \(primitive[A-Za-z0-9]*\)AccessorDepth = \([0-9]\);$/\1:\2/p' "$glue" | sort)
total=$(grep -c '^	{(void\*)_m, ".*", (void\*)[A-Za-z0-9]*},$' "$glue")
test "$declared" = "$names" || { echo "R5: declared: $declared"; rule=FAILED; }
test "$defined" = "$names" || { echo "R5: defined: $defined"; rule=FAILED; }
test "$rows" = "$want" || { echo "R5: rows: $rows"; rule=FAILED; }
test "$depths" = "$want" || { echo "R5: depths: $depths"; rule=FAILED; }
test "$total" = 8 || { echo "R5: $total rows in the table"; rule=FAILED; }
echo "-- R5 (exports): $rule"
test $rule = ok || bad=1

# R7
rule=ok
test "$(cd "$SRCDIR/src/common" && echo *.c)" = SimdCSVPlugin.c || { echo "R7: src/common: $(ls "$SRCDIR/src/common")"; rule=FAILED; }
includes=$(sed -n 's/^#[ \t]*include[ \t]*[<"]\([^>"]*\)[>"].*/\1/p' "$glue" | sort | tr '\n' ' ')
test "$includes" = "scEngine.h sqConfig.h sqPlatformSpecific.h stdint.h string.h virtualMachine.h " \
    || { echo "R7: the glue includes $includes"; rule=FAILED; }
echo "-- R7 (files): $rule"
test $rule = ok || bad=1

case $(uname -s) in
Darwin) platform=osx ;;
*) platform=unix ;;
esac
vminc="-I$vmbuild/build/include/pharovm -I$gen/generated/64/vm/include
 -I$repo/include -I$repo/include/pharovm -I$repo/include/pharovm/common -I$repo/include/pharovm/$platform"
# the definitions every VM source gets (CMakeLists.txt)
vmdefs="-DLSB_FIRST=1 -DIMMUTABILITY=1 -DCOGMTVM=0 -DDEBUGVM=0 -DASYNC_FFI_QUEUE=1 -D_FILE_OFFSET_BITS=64"
glueinc="-I$SRCDIR/include/common"
for std in gnu99 gnu11; do
    for mode in "" -DSQUEAK_BUILTIN_PLUGIN; do
        what="$cc -std=$std -fsyntax-only -Wall -Werror ${mode:-(shared library)}"
        if $cc -std=$std -fsyntax-only -Wall -Werror $vmdefs $mode $glueinc $vminc "$glue"; then
            echo "-- $what: ok"
        else
            failed "$what"
        fi
        what="$what, after windowsNames.h"
        if $cc -std=$std -fsyntax-only -Wall -Werror $vmdefs $mode -include "$SRCDIR/tests/windowsNames.h" $glueinc $vminc "$glue"; then
            echo "-- $what: ok"
        else
            failed "$what"
        fi
    done
done
if ! command -v cmake > /dev/null; then
    failed "genPrimitiveTable.cmake (no cmake)"
elif cmake -DSOURCE="$glue" -DTABLE=SimdCSVPlugin_exports -DOUTPUT="$build/SimdCSVPlugin_primitives.c" \
        -P "$repo/cmake/emscripten/genPrimitiveTable.cmake" \
    && $cc -std=gnu11 -fsyntax-only -Wall -Werror $vmdefs -DSQUEAK_BUILTIN_PLUGIN $glueinc $vminc \
        -I"$SRCDIR/src/common" "$build/SimdCSVPlugin_primitives.c" \
    && test "$(grep -c '^static void w_' "$build/SimdCSVPlugin_primitives.c")" = 4; then
    echo "-- genPrimitiveTable.cmake: 4 trampolines, ok"
else
    failed "genPrimitiveTable.cmake"
fi

asan="-O1 -g -fsanitize=address,undefined -fno-sanitize-recover=all"
warn="-Wall -Wextra -Werror -Wno-unused-parameter -Wno-ignored-qualifiers"
if ! $cxx -std=gnu++17 $asan -fno-exceptions -fno-rtti -fno-strict-aliasing -ffp-contract=off $glueinc "-I$slice" \
        -c "$SRCDIR/src/engine/scEngine.cpp" -o "$build/scEngine.o"; then
    failed "the engine"
else
    for mode in shared builtin; do
        modedefs=
        test $mode = builtin && modedefs="-DSQUEAK_BUILTIN_PLUGIN -DGLUE_BUILTIN"
        objects="$build/scEngine.o"
        for f in src/common/SimdCSVPlugin.c tests/fakevm.c tests/gluetest.c; do
            o=$build/$mode-$(basename "$f" .c).o
            # (the glue converts its Floats through gluetest.c)
            hook=
            test "$f" = src/common/SimdCSVPlugin.c && hook=-Dsc_to_float=test_to_float
            if ! $cc -std=gnu11 $asan $warn $vmdefs $modedefs $hook $glueinc -I"$SRCDIR/tests" $vminc -c "$SRCDIR/$f" -o "$o"; then
                failed "gluetest ($mode), $f"
            fi
            objects="$objects $o"
        done
        # linked as C: the engine needs nothing of C++
        if ! $cc $asan -o "$build/gluetest-$mode" $objects; then
            failed "gluetest ($mode), the link"
            continue
        fi
        for kernel in "" fallback; do
            echo "-- the glue as a $mode plugin, ${kernel:-the best} kernel:"
            SIMDCSV_FORCE_IMPLEMENTATION=$kernel "$build/gluetest-$mode" "$rounds" || failed "gluetest ($mode, ${kernel:-best})"
        done
    done
fi

if test $bad = 0; then
    echo "== glue: ok"
    exit 0
fi
echo "== glue: FAILED"
exit 1
