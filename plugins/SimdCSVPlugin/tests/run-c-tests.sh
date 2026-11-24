#!/bin/sh
# run-c-tests.sh - C-level tests of the SimdCSVPlugin: its engine (src/engine,
# with the slice of simdcsv a9494a8) and its glue (glue.sh)
#
# The script cuts the slice itself, as cmake/simdcsv.cmake does (checked by its
# SHA256), and compiles src/engine/scEngine.cpp with -std=gnu++17
# -fno-exceptions -fno-rtti -fno-strict-aliasing -ffp-contract=off and no -m
# flag (but wasm's -m64).  The engine's lanes define -DSC_TESTING; the VM build
# and the lane symbols do not.  The lanes:
#
#   engine         tests/sctest.cpp on every kernel of the machine: clang++ -O2
#                  -Wall -Wextra -Werror -Wshorten-64-to-32
#   engine-gcc     the same with g++ (without -Wshorten-64-to-32)
#   engine-uchar   the same with clang++ -funsigned-char (AArch64's char)
#   engine-O0      the same with clang++ -O0
#   engine-O3      the same with clang++ -O3 (the VM build's)
#   engine-asan    clang++ -O1 -g, AddressSanitizer and
#                  UndefinedBehaviorSanitizer
#   engine-msan    clang++ -O1 -g, MemorySanitizer
#   guard          SIMDCSV_FORCE_IMPLEMENTATION unset, empty, the kernels',
#                  neon, bogus and 80 x: the kernel and the note of sc_init
#   upstream       simdcsv's own find_indexes, with 64 zero bytes after the
#                  input, against sc_index (x86_64)
#   symbols        the engine with the VM build's flags: hidden sc_* symbols
#                  only, nothing undefined but the list below, no ymm register
#                  outside of sc_avx2, a shared library NEEDED libc only
#                  (Linux); with the glue and --vm-build: the plugin exports
#                  only primitives and module functions
#   wasm64         em++ -O3 -m64 -sMEMORY64=1, without the slice, under node
#   memory64-2     the same with -sMEMORY64=2
#   arm64          (opt-in) aarch64-linux-gnu-g++-13 under qemu -cpu
#                  cortex-a72, both from --arm64-root; no pmull instruction
#   fuzz           (opt-in) tests/fuzz.cpp: libFuzzer, ASan and UBSan, for
#                  --seconds; without libFuzzer its own random inputs
#   4gib           (opt-in) the positions over 4 GiB, and an index of 2^32 - 1
#                  entries (about 21 GiB of memory)
#   glue           tests/glue.sh (its own rules R1, R5 and R7); needs
#                  --vm-build
#
# and checks these static rules (R), on which every lane depends:
#
#   R2  the slice is included by src/engine/scKernels.inc only, once per
#       kernel that holds it
#   R3  no VM header in src/engine nor include/common, no C++ header
#   R6  exactly one getenv in the plugin's sources, in sc_init
#   R8  no printf, long, strtod, allocation, locale nor setlocale in the
#       engine, no -m flag in cmake/simdcsv.cmake but -m64, no vmull_p64 but
#       the macro's and the slice's
#   R11 --only refuses blanks and empty names, which would else select nothing
#       and pass
#   V   the pin: the SHA256 of the archive (when --simdcsv is one), of its
#       LICENSE and src/main.cpp, and of the slice
#
# Options:
#   --simdcsv PATH the archive simdcsv-a9494a8.tar.gz or its tree; default:
#                  $SIMDCSV_SOURCE_DIR, else what the --vm-build tree fetched
#                  (downloads/simdcsv-a9494a8.tar.gz); required otherwise, but
#                  for --only R
#   --emsdk DIR    the Emscripten SDK; default: $EMSDK; without it the lanes
#                  wasm64 and memory64-2 are skipped.  EM_CACHE: its cache,
#                  default <build>/emcache
#   --node PATH    node; default: the newest node of the SDK
#   --vm-build DIR a configured native VM tree, for the lane glue
#   --gen DIR      its GENERATED_SOURCE_DIR; default: read from its
#                  CMakeCache.txt
#   --arm64-root DIR  where usr/bin holds aarch64-linux-gnu-g++-13 and
#                  qemu-aarch64-static (the Ubuntu packages, unpacked)
#   --build DIR    where to build (kept); default: a temporary directory,
#                  removed at the end
#   --only LIST    comma-separated lanes (above) and R, without blanks or empty
#                  names; default: R and every lane that can run here but the
#                  opt-in ones
#   --seconds N    for the lane fuzz, default 60
#   --small        sctest --small everywhere (a quick look)
#
# CC and CXX: the compilers, default clang and clang++.  Nothing is written
# outside of the build directory (and EM_CACHE).  Exits 1 when a lane or a rule
# fails, 2 on a usage error.

set -e

SRCDIR=$(cd "$(dirname "$0")/.." && pwd)
repo=$(cd "$SRCDIR/../.." && pwd)
simdcsv=${SIMDCSV_SOURCE_DIR:-} emsdk=${EMSDK:-} node= vmbuild= gen= build= only= arm64root= seconds=60 small=
while test $# -gt 0; do
    case $1 in
    --simdcsv) simdcsv=$2; shift ;;
    --emsdk) emsdk=$2; shift ;;
    --node) node=$2; shift ;;
    --vm-build) vmbuild=$2; shift ;;
    --gen) gen=$2; shift ;;
    --arm64-root) arm64root=$2; shift ;;
    --build) build=$2; shift ;;
    --only) only=$2; shift ;;
    --seconds) seconds=$2; shift ;;
    --small) small=--small ;;
    *) echo "run-c-tests: unknown option $1" >&2; exit 2 ;;
    esac
    shift
done

all_lanes="engine engine-gcc engine-uchar engine-O0 engine-O3 engine-asan engine-msan guard upstream symbols wasm64 memory64-2 arm64 fuzz 4gib glue"
# selected() matches whole comma-separated names: a blank or an empty name
# would select nothing, and the run would pass with nothing run
case $only in
*[[:space:]]*|,*|*,|*,,*)
    echo "run-c-tests: --only takes names separated by commas, without blanks or empty names, not '$only'" >&2
    exit 2 ;;
esac
for name in $(echo "$only" | tr ',' ' '); do
    case " R $all_lanes " in
    *" $name "*) ;;
    *) echo "run-c-tests: unknown lane $name (R $all_lanes)" >&2; exit 2 ;;
    esac
done
# (rule R11: the options only)
test -z "${SC_OPTIONS_ONLY:-}" || exit 0

selected() {
    test -z "$only" || case ",$only," in *",$1,"*) true ;; *) false ;; esac
}

failed=0
cxx=${CXX:-clang++}

sha256() {
    if command -v sha256sum > /dev/null; then
        sha256sum "$1" | cut -d ' ' -f 1
    else
        shasum -a 256 "$1" | cut -d ' ' -f 1
    fi
}

if test -n "$build"; then
    mkdir -p "$build"
    build=$(cd "$build" && pwd)
else
    build=$(mktemp -d "${TMPDIR:-/tmp}/sctest.XXXXXX")
    trap 'rm -rf "$build"' EXIT
fi

# ---------------------------------------------------------------- the pin
ARCHIVE_SHA256=27eb10cce2765e92244a83ad90698c545ac865ad9e8383cf87d83b6692f256c4
LICENSE_SHA256=c71d239df91726fc519c6eb72d318ec65820627232b2f796219e87dcf35d0ab4
MAIN_SHA256=d9b98882a52d2b018980ee9e6a07625af05be1911a706b60f0a105b2018fc383
SLICE_SHA256=bf5511f156c66cc1c718cac01fd8b32fd5788aa1b6ba7276c0eb546b68d50874
pin=
if test -z "$simdcsv" && test -n "$vmbuild" && test -f "$vmbuild/downloads/simdcsv-a9494a8.tar.gz"; then
    simdcsv=$vmbuild/downloads/simdcsv-a9494a8.tar.gz
fi
pinned() {          # WHAT FILE SHA256
    got=$(sha256 "$2")
    if test "$got" != "$3"; then
        echo "V: $1: SHA256 $got, expected $3"
        pin=bad
    fi
}
if test -n "$simdcsv"; then
    if test -f "$simdcsv"; then
        pinned archive "$simdcsv" $ARCHIVE_SHA256
        if test -z "$pin"; then
            rm -rf "$build/simdcsv"
            mkdir -p "$build/simdcsv"
            tar -xzf "$simdcsv" -C "$build/simdcsv" --strip-components=1
        fi
        simdcsv=$build/simdcsv
    fi
    if test -z "$pin"; then
        if ! test -f "$simdcsv/src/main.cpp" || ! test -f "$simdcsv/LICENSE"; then
            echo "run-c-tests: $simdcsv is not a simdcsv tree (no src/main.cpp, or no LICENSE)" >&2
            exit 2
        fi
        simdcsv=$(cd "$simdcsv" && pwd)
        pinned LICENSE "$simdcsv/LICENSE" $LICENSE_SHA256
        pinned src/main.cpp "$simdcsv/src/main.cpp" $MAIN_SHA256
        mkdir -p "$build/slice" "$build/upstream-inc"
        # bytes 302 to 5390: from "struct simd_input {" to before "//\n// This
        # optimization option might be helpful"
        tail -c +303 "$simdcsv/src/main.cpp" | head -c 5088 > "$build/slice/simdcsv_slice.inc"
        pinned "the slice" "$build/slice/simdcsv_slice.inc" $SLICE_SHA256
        # the whole of it for the lane upstream: ParsedCSV and find_indexes too
        awk '/^struct ParsedCSV/ { p = 1 } /^int main\(/ { exit } p' "$simdcsv/src/main.cpp" \
            > "$build/upstream-inc/simdcsv_upstream.inc"
    fi
fi

# -------------------------------------------------------------- the rules

# The lines of the files that are code: comments and preprocessor lines
# removed, as FILE:LINE: TEXT
code_lines() {
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
           print FILENAME ":" FNR ": " line }' "$@" | sed 's/"\([^"\\]\|\\.\)*"/""/g'
}

rules() {
    engines=$(ls "$SRCDIR"/src/engine/* "$SRCDIR"/include/common/*)
    bad=0
    # R2
    hits=$(grep -l 'simdcsv_slice\.inc"' $engines | tr '\n' ' ')
    n=$(grep -c '^#include "simdcsv_slice\.inc"$' "$SRCDIR/src/engine/scKernels.inc" || true)
    if test "$hits" != "$SRCDIR/src/engine/scKernels.inc " || test "$n" != 2; then
        echo "R2: the slice is included by: $hits($n times in scKernels.inc)"
        bad=1
    fi
    # R3
    if grep -n '^ *# *include' $engines | grep -v -E \
        '<(stddef|stdint|float|stdlib|string|immintrin|arm_neon)\.h>|"(scEngine\.h|scKernels\.inc|scKernelBody\.inc|simdcsv_slice\.inc)"'; then
        echo "R3: an include that is not of the list"
        bad=1
    fi
    # R6
    n=$(code_lines $(find "$SRCDIR/src" "$SRCDIR/include" -type f) | grep -c '[^A-Za-z0-9_]getenv[ \t]*(' || true)
    if test "$n" != 1; then
        echo "R6: $n getenv"
        bad=1
    fi
    # R8
    if code_lines $engines | grep -E '(^|[^A-Za-z0-9_])(printf|fprintf|puts|abort|exit|assert|strto[a-z]*|ato[a-z]*|malloc|calloc|realloc|free|new|delete|throw|try|setlocale|localeconv|alloca)[ \t]*(\(|\[|;)'; then
        echo "R8: a forbidden call"
        bad=1
    fi
    if code_lines $engines | grep -E '(^|[^A-Za-z0-9_])long([^A-Za-z0-9_]|$)'; then
        echo "R8: long (32-bit on Windows): use the sized types"
        bad=1
    fi
    n=$(cat $engines | grep -v '^ \*' | grep 'vmull_p64' | grep -c -v -E '^#(undef vmull_p64|define vmull_p64\(a, b\) sc_prefix_xor\(b\))$' || true)
    if test "$n" != 0; then
        echo "R8: vmull_p64"
        bad=1
    fi
    if test -f "$repo/cmake/simdcsv.cmake" \
        && sed 's/#.*//' "$repo/cmake/simdcsv.cmake" | grep -n -E '(^|[ "(])-m' | grep -v -E '(^|[ "(])-m64([ ")]|$)'; then
        echo "R8: a -m flag in cmake/simdcsv.cmake"
        bad=1
    fi
    # R11
    for o in 'R engine' ' R' 'R,' ',R' 'R,,engine' "$(printf 'R\tengine')" 'R,engine'; do
        want=2
        test "$o" != 'R,engine' || want=0
        ostatus=0
        out=$(SC_OPTIONS_ONLY=1 sh "$SRCDIR/tests/run-c-tests.sh" --only "$o" 2>&1) || ostatus=$?
        if test $ostatus != $want; then
            echo "R11: --only '$o' exits $ostatus, not $want: $out"
            bad=1
        fi
    done
    test -z "$pin" || bad=1
    if test $bad = 0; then
        echo "R2 R3 R6 R8 R11${simdcsv:+ V} ok"
    else
        echo "R FAILED"
        failed=1
    fi
}

if selected R || test -n "$pin"; then
    rules
fi

# -------------------------------------------------------------- the lanes
if test -n "$emsdk" && test -z "$node"; then
    node=$(ls -d "$emsdk"/node/*/bin/node 2>/dev/null | sort -V | tail -n 1)
fi
em=${emsdk:+$emsdk/upstream/emscripten}
machine=$(uname -m)
lanes=
for lane in $all_lanes; do
    if test -n "$only"; then
        selected $lane || continue
    else
        case $lane in
        wasm64|memory64-2) test -n "$emsdk" || { echo "== $lane skipped: no --emsdk"; continue; } ;;
        glue) test -n "$vmbuild" || { echo "== glue skipped: no --vm-build"; continue; } ;;
        upstream) test "$machine" = x86_64 || { echo "== upstream skipped: not x86_64"; continue; } ;;
        arm64|fuzz|4gib) continue ;;
        esac
    fi
    lanes="$lanes $lane"
done
if test $failed != 0; then
    echo "run-c-tests: FAILED (the rules)"
    exit 1
fi
if test -z "$lanes"; then
    if ! selected R; then
        echo "run-c-tests: --only $only selects no lane and no rule" >&2
        exit 2
    fi
    echo "run-c-tests: all lanes passed"
    exit 0
fi
if test -z "$simdcsv"; then
    echo "run-c-tests: --simdcsv PATH is required (the archive simdcsv-a9494a8.tar.gz, or its tree)" >&2
    exit 2
fi

strict="-Wall -Wextra -Werror"
flags="-std=gnu++17 -fno-exceptions -fno-rtti -fno-strict-aliasing -ffp-contract=off"
inc="-I$SRCDIR/include/common"
slice="-I$build/slice"
engine=$SRCDIR/src/engine/scEngine.cpp
asan="-O1 -g -fsanitize=address,undefined -fno-sanitize-recover=all -fno-omit-frame-pointer"
msan="-O1 -g -fsanitize=memory -fno-sanitize-recover=all -fno-omit-frame-pointer"

# build_test NAME COMPILER "FLAGS" "LINK" OUTPUT: the engine and sctest.cpp
build_test() {
    dir=$build/$1
    mkdir -p "$dir"
    $2 $flags $strict $3 -DSC_TESTING $inc $slice -c "$engine" -o "$dir/engine.o" > "$dir/build.log" 2>&1 \
        && $2 $flags $strict $3 -DSC_TESTING $inc $slice -c "$SRCDIR/tests/sctest.cpp" -o "$dir/sctest.o" >> "$dir/build.log" 2>&1 \
        && $2 $3 $4 -o "$dir/$5" "$dir/engine.o" "$dir/sctest.o" >> "$dir/build.log" 2>&1 \
        || { cat "$dir/build.log"; return 1; }
}

# run_sections NAME COMMAND...: every section at the same time
run_sections() {
    dir=$build/$1
    shift
    pids=
    rm -f "$dir"/section-*.log
    names=$("$@" --sections | grep -v asked)
    test -n "$names" || return 1
    for s in $names; do
        "$@" $small $s > "$dir/section-$s.log" 2>&1 &
        pids="$pids $!"
    done
    status=0
    for pid in $pids; do
        wait $pid || status=1
    done
    for s in $names; do
        cat "$dir/section-$s.log"
        grep -q "^$s: [0-9]* checks ok$" "$dir/section-$s.log" || { echo "$s did not finish"; status=1; }
    done
    return $status
}

# test_lane NAME COMPILER FLAGS
test_lane() {
    build_test "$1" "$2" "$3" "" sctest && run_sections "$1" "$build/$1/sctest"
}

wasm_lane() {       # NAME MEMORY64
    if test -z "$em" || ! test -x "$em/em++"; then
        echo "no em++ (--emsdk)"
        return 1
    fi
    if ! test -x "$node"; then
        echo "no node ($node)"
        return 1
    fi
    export EM_CACHE="${EM_CACHE:-$build/emcache}"
    # without the slice: a WebAssembly build contains no simdcsv code
    ( slice=; build_test "$1" "$em/em++" "-O3 -m64 -sMEMORY64=$2 -Wno-unused-command-line-argument" \
        "-sNODERAWFS=1 -sALLOW_MEMORY_GROWTH=1 -sENVIRONMENT=node -sSTACK_SIZE=4MB" sctest.js ) || return 1
    run_sections "$1" "$node" "$build/$1/sctest.js"
}

guard_lane() {
    build_test guard "$cxx" -O2 "" sctest || return 1
    bin=$build/guard/sctest
    names=$(env -u SIMDCSV_FORCE_IMPLEMENTATION "$bin" --forced | cut -d '|' -f 3)
    best=$(echo $names | cut -d ' ' -f 1)
    x55=$(printf '%055d' 0 | tr 0 x)
    ok=0
    expect() {      # WHAT GOT KERNEL NOTE
        if test "$2" = "$3|$4|$names"; then
            echo "-- $1: $3 '$4'"
        else
            echo "-- $1: FAILED: '$2', expected '$3|$4|$names'"
            ok=1
        fi
    }
    expect unset "$(env -u SIMDCSV_FORCE_IMPLEMENTATION "$bin" --forced)" $best ""
    for v in "" avx2 sse2 neon fallback bogus Fallback "fallback " fallbac "$x55" "${x55}x" "${x55}${x55}" "%s%n"; do
        kernel=$best note=
        case " $names " in
        *" $v "*) kernel=$v note=$v ;;
        *) test -z "$v" || note="refused $(printf '%.55s' "$v")" ;;
        esac
        case $v in "") kernel=$best note= ;; "fallback ") kernel=$best note="refused fallback " ;; esac
        expect "'$v'" "$(SIMDCSV_FORCE_IMPLEMENTATION="$v" "$bin" --forced)" "$kernel" "$note"
    done
    # fallback is honoured everywhere
    test "$(SIMDCSV_FORCE_IMPLEMENTATION=fallback "$bin" --forced | cut -d '|' -f 1,2)" = "fallback|fallback" || ok=1
    # and the interface's section passes whatever the environment says
    for v in fallback bogus; do
        SIMDCSV_FORCE_IMPLEMENTATION=$v "$bin" interface || ok=1
    done
    return $ok
}

upstream_lane() {
    if test "$machine" != x86_64; then
        echo "x86_64 only"
        return 1
    fi
    dir=$build/upstream
    mkdir -p "$dir"
    $cxx $flags $strict -O2 -DSC_TESTING $inc $slice -c "$engine" -o "$dir/engine.o" \
        && $cxx $flags -O2 -mavx2 -mbmi -mpclmul -mpopcnt -DSC_UPSTREAM -DSC_TESTING $inc "-I$build/upstream-inc" \
            -c "$SRCDIR/tests/sctest.cpp" -o "$dir/sctest.o" \
        && $cxx -o "$dir/sctest" "$dir/engine.o" "$dir/sctest.o" \
        && "$dir/sctest" $small upstream
}

symbols_lane() {
    dir=$build/symbols
    mkdir -p "$dir"
    status=0
    for c in $cxx g++; do
        command -v "$c" > /dev/null || continue
        o=$dir/engine-$(basename "$c").o
        # cmake/simdcsv.cmake's flags
        $c $flags $strict -O3 -g0 -fPIC -fvisibility=hidden -fvisibility-inlines-hidden -DPHARO_SIMDCSV_VERSION='"a9494a8"' \
            $inc $slice -c "$engine" -o "$o" || return 1
        ok=1
        defined=$(nm -g --defined-only "$o" | awk '{ print $3 }' | sed 's/^_//' | sort | tr '\n' ' ')
        want="sc_cursor_next sc_cursor_open sc_forced_note sc_index sc_init sc_kernel sc_kernel_count sc_kernel_name sc_measure sc_text_copy sc_text_measure sc_to_float sc_to_integer "
        test "$defined" = "$want" || { echo "$c: defined: $defined"; ok=0; }
        for s in $(nm -u "$o" | awk '{ print $NF }' | sed 's/@.*//'); do
            case $s in
            memcpy|memset|memmove|getenv|strlen|_memcpy|_memset|_memmove|_getenv|_strlen) ;;
            __memcpy_chk|__stack_chk_fail|___memcpy_chk|___stack_chk_fail|_GLOBAL_OFFSET_TABLE_) ;;
            __cpu_model|__cpu_indicator_init|___cpu_model|___cpu_indicator_init) ;;
            *) echo "$c: undefined: $s"; ok=0 ;;
            esac
        done
        if test "$(uname -s)" = Linux; then
            if readelf -sW "$o" | awk '$5 == "GLOBAL" && $7 != "UND" && $6 != "HIDDEN" { print $8 }' | grep .; then
                echo "$c: not hidden"
                ok=0
            fi
            if readelf -SW "$o" | grep -E '\.(init_array|ctors|gcc_except_table|tbss|tdata)'; then
                echo "$c: a static constructor, an exception table or a thread local"
                ok=0
            fi
            # linked as C, as the plugin is
            so=$dir/engine-$(basename "$c").so
            ${CC:-clang} -shared -Wl,--no-undefined -o "$so" "$o"
            needed=$(readelf -dW "$so" | sed -n 's/.*(NEEDED).*\[\(.*\)\]/\1/p' | tr '\n' ' ')
            test "$needed" = "libc.so.6 " || { echo "$c: NEEDED $needed"; ok=0; }
            if nm -D --defined-only "$so" | grep -E ' (sc_|_Z)'; then
                echo "$c: exported"
                ok=0
            fi
        fi
        if test "$machine" = x86_64; then
            # (a tzcnt that cannot be given 0 is a bsf on any x86_64)
            objdump -d "$o" | awk '
                /^[0-9a-f]+ <.*>:$/ { fn = $2 }
                /%[yz]mm/ { if (fn ~ /sc_avx2/) avx++; else { print "a ymm register in " fn; bad++ } }
                /[ \t](v?p[a-z]*clmul[a-z]*|vp[a-z]+|popcnt|[a-z]*blsr[a-z]*|andn)[ \t]/ { if (fn !~ /sc_avx2/) { print $0 " in " fn; bad++ } }
                END { if (!avx) print "no ymm register in sc_avx2"; exit bad || !avx }' || { echo "$c: instructions"; ok=0; }
        fi
        # the stack of a call stays under 4 KiB
        if $c -fstack-usage -x c++ -fsyntax-only /dev/null 2>/dev/null; then
            ( cd "$dir" && $c $flags -O3 -fPIC -fstack-usage $inc $slice -c "$engine" -o stack.o )
            big=$(awk -F '\t' '$2 > 1500 { print $1 ": " $2 }' "$dir/stack.su")
            test -z "$big" || { echo "$c: the stack: $big"; ok=0; }
        fi
        if test $ok = 1; then echo "-- $c: ok"; else status=1; fi
    done
    # the plugin itself, when the tree has built it
    if test -n "$vmbuild"; then
        so=$(find "$vmbuild/build" -name 'libSimdCSVPlugin.so' 2>/dev/null | head -n 1)
        if test -n "$so"; then
            exported=$(nm -D --defined-only "$so" | awk '{ print $3 }' | grep -v -E '^(primitive(Info|Measure|Index|Rows)(AccessorDepth)?|getModuleName|initialiseModule|shutdownModule|setInterpreter|_init|_fini|__bss_start|_end|_edata)$' || true)
            needed=$(readelf -dW "$so" | sed -n 's/.*(NEEDED).*\[\(.*\)\]/\1/p' | tr '\n' ' ')
            if test -n "$exported" || test "$needed" != "libc.so.6 "; then
                echo "-- libSimdCSVPlugin.so exports: $exported; NEEDED $needed"
                status=1
            else
                echo "-- $so: ok"
            fi
        fi
    fi
    return $status
}

arm64_lane() {
    if ! test -x "$arm64root/usr/bin/qemu-aarch64-static" || ! test -x "$arm64root/usr/bin/aarch64-linux-gnu-g++-13"; then
        echo "--arm64-root DIR is required (usr/bin/aarch64-linux-gnu-g++-13, usr/bin/qemu-aarch64-static)"
        return 1
    fi
    dir=$build/arm64
    root=$(cd "$arm64root" && pwd)
    mkdir -p "$dir"
    # the host's libraries must not be mixed up with the cross tools'
    ( export LD_LIBRARY_PATH="$root/usr/lib/x86_64-linux-gnu" PATH="$root/usr/bin:$PATH"
      acxx="aarch64-linux-gnu-g++-13 -B$root/usr/lib/gcc-cross/aarch64-linux-gnu/13 -B$root/usr/aarch64-linux-gnu/bin
            -B$root/usr/aarch64-linux-gnu/lib -L$root/usr/aarch64-linux-gnu/lib -static"
      $acxx $flags $strict -O3 -DSC_TESTING $inc $slice -c "$engine" -o "$dir/engine.o" \
        && $acxx $flags $strict -O2 -DSC_TESTING $inc $slice -c "$SRCDIR/tests/sctest.cpp" -o "$dir/sctest.o" \
        && $acxx -o "$dir/sctest" "$dir/engine.o" "$dir/sctest.o" \
        && aarch64-linux-gnu-objdump -d "$dir/engine.o" > "$dir/engine.s" ) || return 1
    if grep -E '[ \t]pmull' "$dir/engine.s"; then
        echo "a pmull instruction"
        return 1
    fi
    set -- "$root/usr/bin/qemu-aarch64-static" -cpu cortex-a72 "$dir/sctest"
    names=$("$@" --forced)
    echo "-- $names"
    case $names in "neon||"*) ;; *) return 1 ;; esac
    run_sections arm64 "$@"
}

fuzz_lane() {
    dir=$build/fuzz
    mkdir -p "$dir/corpus"
    if $cxx $flags $strict $asan -fsanitize=fuzzer -DSC_TESTING $inc $slice -o "$dir/fuzz" "$engine" "$SRCDIR/tests/fuzz.cpp" > "$dir/build.log" 2>&1; then
        ( cd "$dir" && ./fuzz -max_total_time="$seconds" -max_len=600 -print_final_stats=1 corpus > fuzz.log 2>&1 ) \
            || { tail -n 40 "$dir/fuzz.log"; return 1; }
        grep -E 'number_of_executed_units|Done|cov:' "$dir/fuzz.log" | tail -n 3
    else
        echo "-- no libFuzzer: its own inputs only"
    fi
    $cxx $flags $strict $asan -DSC_FUZZ_MAIN -DSC_TESTING $inc $slice -o "$dir/random" "$engine" "$SRCDIR/tests/fuzz.cpp" || return 1
    "$dir/random" --seconds "$seconds"
}

four_gib_lane() {
    build_test 4gib "$cxx" -O2 "" sctest && "$build/4gib/sctest" 4gib
}

glue_lane() {
    if ! test -f "$SRCDIR/tests/glue.sh"; then
        echo "no glue.sh"
        return 1
    fi
    sh "$SRCDIR/tests/glue.sh" --vm-build "$vmbuild" ${gen:+--gen "$gen"} --slice "$build/slice" --build "$build/glue"
}

export ASAN_OPTIONS=${ASAN_OPTIONS:-detect_leaks=1} UBSAN_OPTIONS=${UBSAN_OPTIONS:-print_stacktrace=1}
for name in $lanes; do
    echo "== $name"
    case $name in
    engine) set -- test_lane engine "$cxx" "-O2 -Wshorten-64-to-32" ;;
    engine-gcc) set -- test_lane engine-gcc g++ -O2 ;;
    engine-uchar) set -- test_lane engine-uchar "$cxx" "-O2 -Wshorten-64-to-32 -funsigned-char" ;;
    engine-O0) set -- test_lane engine-O0 "$cxx" -O0 ;;
    engine-O3) set -- test_lane engine-O3 "$cxx" -O3 ;;
    engine-asan) set -- test_lane engine-asan "$cxx" "$asan" ;;
    engine-msan) set -- test_lane engine-msan "$cxx" "$msan" ;;
    guard) set -- guard_lane ;;
    upstream) set -- upstream_lane ;;
    symbols) set -- symbols_lane ;;
    wasm64) set -- wasm_lane wasm64 1 ;;
    memory64-2) set -- wasm_lane memory64-2 2 ;;
    arm64) set -- arm64_lane ;;
    fuzz) set -- fuzz_lane ;;
    4gib) set -- four_gib_lane ;;
    glue) set -- glue_lane ;;
    esac
    if "$@"; then
        echo "== $name: ok"
    else
        echo "== $name: FAILED"
        failed=1
    fi
done

if test $failed = 0; then
    echo "run-c-tests: all lanes passed"
else
    echo "run-c-tests: FAILED"
fi
exit $failed
