#!/bin/sh
# run-c-tests.sh - C-level tests of the CairoPlugin (docs/CairoPlugin.md):
# static rules, its binding layer against real cairo builds and headers, and
# its glue against a fake VM, natively and as the wasm64 builtin plugin.
#
# The lanes:
#
#   R            static rules (below)
#   bind         tests/bindtest.c + src/binding/cpCairo.c (own types, as the
#                native builds): a dlsym loader on a bundled libcairo.so.2
#                (--libcairo, default <--vm-build>/build/vm/libcairo.so.2):
#                every function bound, matrices, drawing read back, text
#   protocheck   two translation units, host clang -fsyntax-only
#                -Werror=incompatible-function-pointer-types
#                -Werror=incompatible-pointer-types, against the cairo
#                1.18.4 headers of --deps (cairo-1.18.4.tar.xz, with the
#                repository's cmake/emscripten/deps/cairo/cairo-features.h,
#                and the FreeType headers of freetype-2.14.3.tar.xz):
#                (1) cpCairo.c with -DCP_HAVE_CAIRO_H -DCP_CAIRO_TYPES
#                -DCP_STATIC_CAIRO (the Emscripten mode); (2)
#                tests/protocheck-own.c (the own types of the native builds,
#                field by field and function by function)
#   targets      clang --target=T -ffreestanding -fsyntax-only of cpCairo.h,
#                cpCairo.c and protocheck-own.c -DCP_PROTOCHECK_NO_FT, for
#                every shipped ABI (x86_64/aarch64 Linux, arm64/x86_64 macOS,
#                x86_64/aarch64 MinGW, wasm64 Emscripten): the struct layouts
#                and prototypes where no CI runs here; needs the cairo headers
#                of --deps
#   symbols      every CP_REQ/CP_OPT/CP_PROBE function is exported by the
#                libcairo of --vm-build (nm -D) and by the other platforms'
#                bundles found in --deps (*_third-party_cairo-*.zip: Linux
#                .so with llvm-nm -D, macOS .dylib with llvm-nm, Windows .dll
#                with llvm-readobj --coff-exports: the files.pharo.org
#                bundles; the MinGW zips of build.yml ship MSYS2's
#                libcairo-2.dll instead, which only their CI step,
#                tests/ci-load.st, checks); libCairoPlugin.so of
#                --vm-build exports exactly the module functions, the
#                primitives of tests/primitives.txt and their *AccessorDepth
#                variables (no su_, no simdutf, no cp_ symbol)
#   wasm-syntax  emcc -fsyntax-only -Wall -Wextra -Werror of CairoPlugin.c
#                (with src/prims) and cpCairo.c with the flags of the
#                CairoPlugin target of --wasm-build (-DSQUEAK_BUILTIN_PLUGIN
#                -DCP_HAVE_CAIRO_H -DCP_CAIRO_TYPES -DCP_STATIC_CAIRO, the
#                cairo and VM headers); then
#                cmake/emscripten/genPrimitiveTable.cmake on CairoPlugin.c and
#                the wrapper it writes compiled the same way
#   glue-admin   tests/glue-admin.c (which #includes CairoPlugin.c) +
#                tests/fakevm.c + cpCairo.c + the SimdUTF engine of
#                --vm-build (deps/libpharo_su_engine.a, libpharo_simdutf.a):
#                first CairoPlugin.c -fsyntax-only -Wall -Wextra -Werror as a
#                shared library's glue and as a builtin plugin's, then the test
#                with clang -O1 ASan and UBSan in both modes, on the bundled
#                libcairo (--libcairo); also genPrimitiveTable.cmake
#   glue-basic   tests/glue-basic.c (WP-B), the same way
#   glue-shape   tests/glue-shape.c (WP-C), the same way
#   glue-text    tests/glue-text.c (WP-C), the same way
#   glue-wasm64  every tests/glue-*.c, emcc -m64 -sMEMORY64=1
#                -sSUPPORT_LONGJMP=wasm, builtin, with the flags of
#                --wasm-build's CairoPlugin, linked with its
#                libpharo_{cairo,pixman,png,freetype,zlib}.a and engine
#                (libpharo_su_engine.a, libpharo_simdutf.a), run in node: the
#                static table and the builtin glue as the web VMs have them
#
# A lane whose test file does not exist yet prints "not written yet
# (WP-x)" and passes.
#
# The static rules (R):
#   R1  allocation only through the helpers: instantiateClassindexableSize
#       only in cp_instantiate, floatObjectOf only in cp_new_float; never
#       stringForCString, clone, signed64BitIntegerFor,
#       positive64BitIntegerFor, methodReturnString; the C heap (malloc,
#       calloc, realloc, free) only in src/prims/text.c and src/prims/shape.c
#   R2  cairo.h (cairo-ft.h, any cairo*.h) is included only by
#       include/common/cpCairo.h, under #ifdef CP_HAVE_CAIRO_H
#   R3  no cp_api. outside the CP_CALL and CP_HAVE macros
#   R4  src/common holds exactly one .c file, CairoPlugin.c
#   R5  the primitives of tests/primitives.txt (69), the CP_PRIMITIVES list
#       (in its order), the declarations and the definitions (each in the
#       file primitives.txt says), the rows of the builtin table and the
#       *AccessorDepth variables (with the depths) are the same
#   R6  no printf, fprintf, puts, putchar, perror, exit, abort in src
#   R7  isBytes only inside cp_bytes
#   R8  no storePointerUnchecked
#   R9  the module loader: ioLoadSymbolOfLengthFromModule only in
#       cp_vm_loader, which tests its module for NULL first; in
#       primitiveBind the module is tested for NULL after
#       ioLoadModuleOfLength and before cp_cairo_bind
#
# Environment: GLUE_FLAGS (extra flags of the glue tests) and CP_TEST_FONT
# (the TrueType font of glue-basic and glue-text; a path without spaces).
#
# Options:
#   --vm-build DIR    a configured and built native VM tree (its headers,
#                     build/vm/libcairo.so.2, build/vm/libCairoPlugin.so,
#                     deps/libpharo_su_engine.a and libpharo_simdutf.a)
#   --gen DIR         its GENERATED_SOURCE_DIR (default: from its cache)
#   --libcairo PATH   the libcairo of the bind and glue lanes (default:
#                     <vm-build>/build/vm/libcairo.so.2; its directory goes
#                     to LD_LIBRARY_PATH)
#   --deps DIR        the archives (cairo-1.18.4.tar.xz,
#                     freetype-2.14.3.tar.xz, *_third-party_cairo-*.zip)
#   --emsdk DIR       the Emscripten SDK (default: $EMSDK)
#   --node PATH       node for glue-wasm64 (default: the newest of the SDK)
#   --wasm-build DIR  a built Emscripten tree (WASM_BUILDDIR, with cmake/)
#   --build DIR       where to build (kept); default: a temporary directory
#   --only LIST       comma-separated lanes, e.g. R,bind,glue-admin;
#                     default: every lane whose inputs were given
#
# Nothing is written outside the build directory.  Exits 1 when a lane or
# a rule fails, 2 on a usage error.

set -e

# extra compiler flags for the glue tests (e.g. GLUE_FLAGS=-DGB_DEPTH_DELTA=-1),
# from the environment
GLUE_FLAGS=${GLUE_FLAGS:-}
# CP_TEST_FONT (environment): the TrueType font of glue-basic and glue-text
# (--font), for hosts without DejaVu, Liberation or Arial
CP_TEST_FONT=${CP_TEST_FONT:-}

# the --font argument of a glue test (only glue-basic and glue-text take it)
font_args() {
    case $1 in
    glue-basic|glue-text) test -n "$CP_TEST_FONT" && printf '%s\n' "--font" "$CP_TEST_FONT" ;;
    esac
    return 0
}

SRCDIR=$(cd "$(dirname "$0")/.." && pwd)
REPO=$(cd "$SRCDIR/../.." && pwd)
vmbuild= gen= libcairo= deps= emsdk=${EMSDK:-} node= wasmbuild= build= only=
while test $# -gt 0; do
    case $1 in
    --vm-build) vmbuild=$2; shift ;;
    --gen) gen=$2; shift ;;
    --libcairo) libcairo=$2; shift ;;
    --deps) deps=$2; shift ;;
    --emsdk) emsdk=$2; shift ;;
    --node) node=$2; shift ;;
    --wasm-build) wasmbuild=$2; shift ;;
    --build) build=$2; shift ;;
    --only) only=$2; shift ;;
    *) echo "run-c-tests: unknown option $1" >&2; exit 2 ;;
    esac
    shift
done

all_lanes="bind protocheck targets symbols wasm-syntax glue-admin glue-basic glue-shape glue-text glue-wasm64"
for name in $(echo "$only" | tr ',' ' '); do
    case " R $all_lanes " in
    *" $name "*) ;;
    *) echo "run-c-tests: unknown lane $name (R $all_lanes)" >&2; exit 2 ;;
    esac
done
selected() {
    test -z "$only" || case ",$only," in *",$1,"*) true ;; *) false ;; esac
}

absdir() { (cd "$1" && pwd); }
test -n "$vmbuild" && vmbuild=$(absdir "$vmbuild")
test -n "$deps" && deps=$(absdir "$deps")
test -n "$wasmbuild" && wasmbuild=$(absdir "$wasmbuild")
if test -z "$libcairo" && test -n "$vmbuild" && test -e "$vmbuild/build/vm/libcairo.so.2"; then
    libcairo=$vmbuild/build/vm/libcairo.so.2
fi
if test -n "$emsdk" && test -z "$node"; then
    node=$(ls -d "$emsdk"/node/*/bin/node 2>/dev/null | sort -V | tail -n 1)
fi
em=
test -n "$emsdk" && em=$emsdk/upstream/emscripten
cc=${CC:-clang}
cxx=${CXX:-clang++}
llvmbin=
for v in "" -20 -19 -18; do
    if command -v llvm-nm$v > /dev/null 2>&1; then llvmbin=$v; break; fi
done

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
           gsub(/"([^"\\]|\\.)*"/, "\"\"", line)
           print FILENAME ":" FNR ": " line }' "$@"
}

# The functions in which a word occurs (code lines): FILE:FUNCTION per
# occurrence; a function starts at a line "name(" at column 0 that does not
# end with ';' and ends at "}" at column 0
sites_of() {
    word=$1
    shift
    code_lines "$@" | awk -v w="$word" '
        { file = $0; sub(/:.*/, "", file); text = $0; sub(/^[^:]*:[0-9]+: /, "", text) }
        text ~ /^[A-Za-z_][A-Za-z0-9_]*\(/ && text !~ /;[ \t]*$/ { fn = text; sub(/\(.*/, "", fn) }
        text ~ /^}/ { fn = "" }
        { n = 0; t = text
          while (match(t, "(^|[^A-Za-z0-9_])" w "[ \t]*\\(")) { n++; t = substr(t, RSTART + RLENGTH) }
          for (i = 0; i < n; i++) print file ":" (fn == "" ? "?" : fn) }'
}

# The primitives of the contract
prims_txt=$SRCDIR/tests/primitives.txt
glue=$SRCDIR/src/common/CairoPlugin.c
primfiles="$SRCDIR/src/prims/basic.c $SRCDIR/src/prims/shape.c $SRCDIR/src/prims/text.c"
binding=$SRCDIR/src/binding/cpCairo.c

rules() {
    bad=0
    srcs="$glue $primfiles"
    # R1
    for word in stringForCString clone signed64BitIntegerFor positive64BitIntegerFor methodReturnString storePointerUnchecked; do
        if sites_of "$word" $srcs | grep -q .; then
            echo "R1/R8: $word is called in:" $(sites_of "$word" $srcs)
            bad=1
        fi
    done
    s=$(sites_of instantiateClassindexableSize $srcs | sed 's|.*/||' | sort -u)
    if test "$s" != "CairoPlugin.c:cp_instantiate"; then
        echo "R1: instantiateClassindexableSize is called in:" $s
        bad=1
    fi
    s=$(sites_of floatObjectOf $srcs | sed 's|.*/||' | sort -u)
    if test -n "$s" && test "$s" != "CairoPlugin.c:cp_new_float"; then
        echo "R1: floatObjectOf is called in:" $s
        bad=1
    fi
    for word in malloc calloc realloc free; do
        s=$(sites_of "$word" $srcs | sed 's|.*/||; s|:.*||' | grep -v -E '^(text|shape)\.c$' || true)
        if test -n "$s"; then
            echo "R1: $word is called in:" $s
            bad=1
        fi
    done
    # R2
    for f in $(find "$SRCDIR/src" "$SRCDIR/include" "$SRCDIR/tests" -name '*.[ch]'); do
        case $f in
        */include/common/cpCairo.h)
            if ! awk '/^#ifdef CP_HAVE_CAIRO_H/ { depth = 1; next }
                      depth && /^#if/ { depth++ } depth && /^#endif/ { depth--; next }
                      /#include *[<"]cairo/ && !depth { bad = 1 } END { exit bad }' "$f"; then
                echo "R2: $f includes cairo.h outside #ifdef CP_HAVE_CAIRO_H"
                bad=1
            fi ;;
        *)
            if grep -n '#include *[<"]cairo[a-z-]*\.h[>"]' "$f"; then
                echo "R2: $f includes a cairo header"
                bad=1
            fi ;;
        esac
    done
    # R3
    if code_lines $srcs | grep 'cp_api\.'; then
        echo "R3: cp_api. outside CP_CALL and CP_HAVE"
        bad=1
    fi
    if ! grep -q '^#define CP_CALL(name) (cp_cairo_calls\[CP_FN_##name\]++, cp_api\.name)$' "$glue"; then
        echo "R3: CP_CALL is not the counted call"
        bad=1
    fi
    # R4
    commons=$(cd "$SRCDIR/src/common" && ls *.c 2>/dev/null) || true
    if test "$commons" != CairoPlugin.c; then
        echo "R4: src/common holds:" $commons
        bad=1
    fi
    # R5
    want=$(awk '/^primitive/ { print $1 ":" $2 }' "$prims_txt")
    names=$(echo "$want" | sed 's/:.*//')
    count=$(echo "$names" | grep -c .)
    if test "$count" != 69; then
        echo "R5: primitives.txt lists $count primitives, not 69"
        bad=1
    fi
    plist=$(sed -n 's/^	P(\([A-Za-z0-9]*\)).*/primitive\1/p' "$glue")
    if test "$plist" != "$names"; then
        echo "R5: the CP_PRIMITIVES list differs from primitives.txt"
        bad=1
    fi
    declared=$(sed -n 's/^EXPORT(sqInt) \(primitive[A-Za-z0-9]*\)(void);$/\1/p' "$glue")
    if test "$declared" != "$names"; then
        echo "R5: the declarations differ from primitives.txt (order included)"
        bad=1
    fi
    defined=$(for f in $glue $primfiles; do
        awk -v file="${f#$SRCDIR/}" '/^EXPORT\(sqInt\)$/ { getline; if (match($0, /^primitive[A-Za-z0-9]*\(void\)$/)) { sub(/\(.*/, ""); print $0 " " file } }' "$f"
    done | sort)
    wantdefined=$(awk '/^primitive/ { print $1 " " $4 }' "$prims_txt" | sort)
    if test "$defined" != "$wantdefined"; then
        echo "R5: the definitions (and their files) differ from primitives.txt:"
        echo "$defined" > "${TMPDIR:-/tmp}/cp-r5-defined.$$"
        echo "$wantdefined" | diff - "${TMPDIR:-/tmp}/cp-r5-defined.$$" | head -10
        rm -f "${TMPDIR:-/tmp}/cp-r5-defined.$$"
        bad=1
    fi
    rows=$(sed -n 's/^	{(void\*)_m, "\(primitive[A-Za-z0-9]*\)\\000\\\([0-7][0-7][0-7]\)", (void\*)\(primitive[A-Za-z0-9]*\)},$/\1 \2 \3/p' "$glue" \
        | awk '{ d = 0; for (i = 1; i <= length($2); i++) d = d * 8 + substr($2, i, 1); if ($1 == $3) print $1 ":" d; else print "mismatch:" $1 "/" $3 }')
    if test "$rows" != "$want"; then
        echo "R5: the rows of CairoPlugin_exports differ from primitives.txt"
        bad=1
    fi
    depths=$(sed -n 's/^EXPORT(signed char) \(primitive[A-Za-z0-9]*\)AccessorDepth = \([0-9]\);$/\1:\2/p' "$glue")
    if test "$depths" != "$want"; then
        echo "R5: the *AccessorDepth variables differ from primitives.txt"
        bad=1
    fi
    # R6
    for word in printf fprintf puts putchar perror exit abort; do
        if sites_of "$word" $srcs $binding | grep -q .; then
            echo "R6: $word is called in:" $(sites_of "$word" $srcs $binding)
            bad=1
        fi
    done
    # R7
    s=$(sites_of isBytes $srcs | sed 's|.*/||' | sort -u)
    if test "$s" != "CairoPlugin.c:cp_bytes"; then
        echo "R7: isBytes is called in:" $s
        bad=1
    fi
    # R9
    s=$(sites_of ioLoadSymbolOfLengthFromModule $srcs | sed 's|.*/||' | sort -u)
    if test "$s" != "CairoPlugin.c:cp_vm_loader"; then
        echo "R9: ioLoadSymbolOfLengthFromModule is called in:" $s
        bad=1
    fi
    if ! code_lines "$glue" | awk '
            { text = $0; sub(/^[^:]*:[0-9]+: /, "", text) }
            text ~ /^cp_vm_loader\(/ { inl = 1; next }
            inl && text ~ /^}/ { inl = 0 }
            inl && !tested && text ~ /if \(!module\)/ { tested = 1 }
            inl && text ~ /ioLoadSymbolOfLengthFromModule/ && !tested { early = 1 }
            text ~ /^primitiveBind\(/ { inb = 1; next }
            inb && text ~ /^}/ { inb = 0 }
            inb && text ~ /ioLoadModuleOfLength/ { loaded = NR }
            inb && loaded && !check && text ~ /if \(!module\)/ { check = NR }
            inb && text ~ /cp_cairo_bind\(/ { bound = NR }
            END { exit !(tested && !early && loaded && check > loaded && bound > check) }'; then
        echo "R9: the NULL module is not refused before every symbol lookup"
        bad=1
    fi
    if test $bad = 0; then
        echo "R1 R2 R3 R4 R5 R6 R7 R8 R9 ok ($count primitives)"
    else
        echo "R FAILED"
        failed=1
    fi
}

if selected R; then
    echo "== R"
    rules
fi

# the lanes to run
lanes=
for lane in $all_lanes; do
    if test -n "$only"; then
        selected $lane || continue
    else
        case $lane in
        bind|glue-admin|glue-basic|glue-shape|glue-text)
            test -n "$vmbuild" || { echo "== $lane skipped: no --vm-build"; continue; } ;;
        protocheck|targets)
            test -n "$deps" || { echo "== $lane skipped: no --deps"; continue; } ;;
        symbols)
            test -n "$vmbuild" || { echo "== $lane skipped: no --vm-build"; continue; } ;;
        wasm-syntax|glue-wasm64)
            { test -n "$em" && test -n "$wasmbuild"; } || { echo "== $lane skipped: no --emsdk and --wasm-build"; continue; } ;;
        esac
    fi
    lanes="$lanes $lane"
done
if test -z "$lanes"; then
    test $failed = 0 && echo "run-c-tests: all lanes passed" || echo "run-c-tests: FAILED"
    exit $failed
fi
if test -n "$build"; then
    mkdir -p "$build"
    build=$(absdir "$build")
else
    build=$(mktemp -d "${TMPDIR:-/tmp}/cptest.XXXXXX")
    trap 'rm -rf "$build"' EXIT
fi

asan="-fsanitize=address,undefined -fno-sanitize-recover=undefined -fno-omit-frame-pointer"
# -Wextra, but not on the VM's virtualMachine.h (its volatile return type)
strict="-Wall -Wextra -Wno-ignored-qualifiers -Werror"
incs="-I$SRCDIR/include/common -I$SRCDIR/tests -I$SRCDIR/src/common -I$REPO/plugins/SimdUTFPlugin/include/common"

# the cairo 1.18.4 and FreeType headers of --deps, extracted once
headers() {
    hdir=$build/headers
    if ! test -f "$hdir/.done"; then
        rm -rf "$hdir"
        mkdir -p "$hdir"
        if ! test -f "$deps/cairo-1.18.4.tar.xz" || ! test -f "$deps/freetype-2.14.3.tar.xz"; then
            echo "-- no cairo-1.18.4.tar.xz or freetype-2.14.3.tar.xz in --deps $deps"
            return 1
        fi
        tar -xJf "$deps/cairo-1.18.4.tar.xz" -C "$hdir" --wildcards 'cairo-1.18.4/src/*.h' || return 1
        tar -xJf "$deps/freetype-2.14.3.tar.xz" -C "$hdir" --wildcards 'freetype-2.14.3/include/*' || return 1
        touch "$hdir/.done"
    fi
    cairoinc="-I$hdir/cairo-1.18.4/src -I$REPO/cmake/emscripten/deps/cairo"
    ftinc="-I$hdir/freetype-2.14.3/include"
}

lane_ok() { echo "== $1: ok"; }
lane_failed() { echo "== $1: FAILED${2:+ ($2)}"; failed=1; }

lane_bind() {
    echo "== bind"
    if test -z "$libcairo" || ! test -e "$libcairo"; then
        lane_failed bind "no libcairo (--libcairo or --vm-build)"
        return
    fi
    dir=$build/bind
    mkdir -p "$dir"
    if ! $cc -std=gnu11 -O1 -g $asan -Wall -Wextra -Werror $incs -o "$dir/bindtest" \
            "$SRCDIR/tests/bindtest.c" "$binding" -ldl -lm > "$dir/build.log" 2>&1; then
        cat "$dir/build.log"
        lane_failed bind build
        return
    fi
    if LD_LIBRARY_PATH=$(dirname "$libcairo") "$dir/bindtest" --libcairo "$libcairo"; then
        lane_ok bind
    else
        lane_failed bind
    fi
}

lane_protocheck() {
    echo "== protocheck"
    headers || { lane_failed protocheck "headers"; return; }
    ok=1
    pflags="-std=gnu11 -fsyntax-only -Wall -Wextra -Werror -Werror=incompatible-function-pointer-types -Werror=incompatible-pointer-types"
    if $cc $pflags -DCP_HAVE_CAIRO_H -DCP_CAIRO_TYPES -DCP_STATIC_CAIRO $incs $cairoinc $ftinc "$binding"; then
        echo "-- cpCairo.c, cairo types and the static table (the Emscripten mode): ok"
    else
        echo "-- cpCairo.c, cairo types and the static table: FAILED"
        ok=0
    fi
    if $cc $pflags $incs $cairoinc $ftinc "$SRCDIR/tests/protocheck-own.c"; then
        echo "-- protocheck-own.c, the own types against cairo.h and cairo-ft.h: ok"
    else
        echo "-- protocheck-own.c: FAILED"
        ok=0
    fi
    test $ok = 1 && lane_ok protocheck || lane_failed protocheck
}

lane_targets() {
    echo "== targets"
    headers || { lane_failed targets "headers"; return; }
    ok=1
    for t in x86_64-linux-gnu aarch64-linux-gnu arm64-apple-macos x86_64-apple-macos \
             x86_64-w64-mingw32 aarch64-w64-mingw32 wasm64-unknown-emscripten; do
        tflags="--target=$t -ffreestanding -std=c11 -fsyntax-only -Wall -Werror -Werror=incompatible-function-pointer-types -Werror=incompatible-pointer-types"
        if $cc $tflags $incs -x c "$SRCDIR/include/common/cpCairo.h" \
            && $cc $tflags $incs "$binding" \
            && $cc $tflags -DCP_PROTOCHECK_NO_FT $incs $cairoinc "$SRCDIR/tests/protocheck-own.c"; then
            echo "-- $t: ok"
        else
            echo "-- $t: FAILED"
            ok=0
        fi
    done
    test $ok = 1 && lane_ok targets || lane_failed targets
}

# the names of the functions of cpCairo.h
cairo_names() {
    sed -n 's/^	X([^,]*, \([a-z0-9_]*\), .*/cairo_\1/p' "$SRCDIR/include/common/cpCairo.h"
}

# check_exports FILE TOOL-OUTPUT: every cairo name is in the exports
check_exports() {
    what=$1 exports=$2
    missing=
    for n in $(cairo_names); do
        echo "$exports" | grep -qx "$n" || missing="$missing $n"
    done
    if test -n "$missing"; then
        echo "-- $what: missing$missing"
        return 1
    fi
    echo "-- $what: all $(cairo_names | wc -l) functions exported"
}

lane_symbols() {
    echo "== symbols"
    ok=1
    n=$(cairo_names | wc -l)
    if test "$n" != 57; then
        echo "-- cpCairo.h lists $n functions, not 57"
        ok=0
    fi
    if test -n "$libcairo" && test -e "$libcairo"; then
        check_exports "$libcairo" "$(nm -D --defined-only "$libcairo" | awk '{ print $3 }' | sed 's/@.*//')" || ok=0
    else
        echo "-- no libcairo of --vm-build"
        ok=0
    fi
    dir=$build/symbols
    mkdir -p "$dir"
    if test -n "$deps"; then
        for zip in "$deps"/*_third-party_cairo-*.zip; do
            test -f "$zip" || continue
            z=$(basename "$zip" .zip)
            mkdir -p "$dir/$z"
            unzip -q -o "$zip" -d "$dir/$z" || { echo "-- $z: cannot unzip"; ok=0; continue; }
            for lib in $(find "$dir/$z" -type f \( -name 'libcairo.so.*' -o -name 'libcairo.*.dylib' -o -name 'libcairo-2.dll' \)); do
                case $lib in
                *.dll) exports=$(llvm-readobj$llvmbin --coff-exports "$lib" | sed -n 's/^ *Name: //p') ;;
                *.dylib) exports=$(llvm-nm$llvmbin -gU "$lib" | awk '{ print $3 }' | sed 's/^_//') ;;
                *) exports=$(llvm-nm$llvmbin -D --defined-only "$lib" | awk '{ print $3 }' | sed 's/@.*//') ;;
                esac
                check_exports "$z/$(basename "$lib")" "$exports" || ok=0
            done
        done
    fi
    plugin=$vmbuild/build/vm/libCairoPlugin.so
    if test -f "$plugin"; then
        want=$( (awk '/^primitive/ { print $1; print $1 "AccessorDepth" }' "$prims_txt";
                 printf '%s\n' getModuleName initialiseModule setInterpreter shutdownModule) | sort)
        have=$(nm -D --defined-only "$plugin" | awk '$2 ~ /^[TDBRVWiu]$/ { print $3 }' | sed 's/@.*//' | sort)
        if test "$want" = "$have"; then
            echo "-- libCairoPlugin.so exports exactly $(echo "$have" | wc -l) symbols: the module functions, the 69 primitives and their depths"
        else
            echo "-- libCairoPlugin.so exports differ (< expected, > exported):"
            echo "$want" > "$dir/want.txt"
            echo "$have" > "$dir/have.txt"
            diff "$dir/want.txt" "$dir/have.txt" | grep '^[<>]' | head -20
            ok=0
        fi
        if nm -D --defined-only "$plugin" | grep -E ' (su_|cp_|.*simdutf)'; then
            echo "-- libCairoPlugin.so exports engine or binding symbols"
            ok=0
        fi
    else
        echo "-- no $plugin"
        ok=0
    fi
    test $ok = 1 && lane_ok symbols || lane_failed symbols
}

# The flags of the CairoPlugin target of --wasm-build (flags.make)
wasm_flags() {
    fm=$wasmbuild/cmake/CMakeFiles/CairoPlugin.dir/flags.make
    if ! test -f "$fm"; then
        echo "-- no $fm (--wasm-build: a built WASM_BUILDDIR)"
        return 1
    fi
    wdefs=$(sed -n 's/^C_DEFINES = //p' "$fm")
    wincs=$(sed -n 's/^C_INCLUDES = //p' "$fm")
}

lane_wasm_syntax() {
    echo "== wasm-syntax"
    wasm_flags || { lane_failed wasm-syntax; return; }
    dir=$build/wasm-syntax
    mkdir -p "$dir"
    ok=1
    sflags="-m64 -std=gnu11 -fsyntax-only $strict -Werror=incompatible-function-pointer-types -Werror=incompatible-pointer-types"
    if "$em/emcc" $sflags $wdefs $wincs $incs "$glue" && "$em/emcc" $sflags $wdefs $wincs $incs "$binding"; then
        echo "-- emcc -fsyntax-only CairoPlugin.c (src/prims) and cpCairo.c: ok"
    else
        echo "-- emcc -fsyntax-only: FAILED"
        ok=0
    fi
    if cmake -DSOURCE="$glue" -DTABLE=CairoPlugin_exports -DOUTPUT="$dir/CairoPlugin_primitives.c" \
            -P "$REPO/cmake/emscripten/genPrimitiveTable.cmake" \
        && "$em/emcc" $sflags $wdefs $wincs $incs "$dir/CairoPlugin_primitives.c"; then
        echo "-- genPrimitiveTable.cmake: $(grep -c '^static void w_' "$dir/CairoPlugin_primitives.c") trampolines, ok"
    else
        echo "-- genPrimitiveTable.cmake: FAILED"
        ok=0
    fi
    test $ok = 1 && lane_ok wasm-syntax || lane_failed wasm-syntax
}

# The headers of the native VM tree
vm_headers() {
    if test -z "$vmbuild" || ! test -f "$vmbuild/CMakeCache.txt"; then
        echo "-- --vm-build DIR, a configured VM tree, is required"
        return 1
    fi
    if test -z "$gen"; then
        gen=$(sed -n 's/^GENERATED_SOURCE_DIR:[A-Z]*=//p' "$vmbuild/CMakeCache.txt")
        test -n "$gen" || gen=$vmbuild
    fi
    case $(uname -s) in
    Darwin) platform=osx ;;
    *) platform=unix ;;
    esac
    vminc="-I$vmbuild/build/include/pharovm -I$gen/generated/64/vm/include
 -I$REPO/include -I$REPO/include/pharovm -I$REPO/include/pharovm/common -I$REPO/include/pharovm/$platform"
    # the definitions every VM source gets (CMakeLists.txt)
    vmdefs="-DLSB_FIRST=1 -DIMMUTABILITY=1 -DCOGMTVM=0 -DDEBUGVM=0 -DASYNC_FFI_QUEUE=1 -D_FILE_OFFSET_BITS=64"
    engine="$vmbuild/deps/libpharo_su_engine.a $vmbuild/deps/libpharo_simdutf.a"
    for f in "$vmbuild/build/include/pharovm/config.h" "$gen/generated/64/vm/include/interp.h" $engine; do
        if ! test -f "$f"; then
            echo "-- no $f (--vm-build, --gen)"
            return 1
        fi
    done
}

# lane_glue NAME OWNER: tests/NAME.c against the fake VM, shared and builtin
lane_glue() {
    name=$1 owner=$2
    echo "== $name"
    test_c=$SRCDIR/tests/$name.c
    if ! test -f "$test_c"; then
        echo "-- not written yet ($owner)"
        lane_ok "$name"
        return
    fi
    vm_headers || { lane_failed "$name"; return; }
    if test -z "$libcairo" || ! test -e "$libcairo"; then
        lane_failed "$name" "no libcairo"
        return
    fi
    dir=$build/$name
    mkdir -p "$dir"
    ok=1
    for mode in "" -DSQUEAK_BUILTIN_PLUGIN; do
        if $cc -std=gnu11 -fsyntax-only $strict $vmdefs $mode $incs $vminc "$glue"; then
            echo "-- CairoPlugin.c -fsyntax-only -Wall -Wextra -Werror ${mode:-(shared library)}: ok"
        else
            echo "-- CairoPlugin.c -fsyntax-only -Wall -Wextra -Werror ${mode:-(shared library)}: FAILED"
            ok=0
        fi
    done
    if test "$name" = glue-admin; then
        if cmake -DSOURCE="$glue" -DTABLE=CairoPlugin_exports -DOUTPUT="$dir/CairoPlugin_primitives.c" \
                -P "$REPO/cmake/emscripten/genPrimitiveTable.cmake" \
            && $cc -std=gnu11 -fsyntax-only $strict $vmdefs -DSQUEAK_BUILTIN_PLUGIN $incs $vminc \
                "$dir/CairoPlugin_primitives.c"; then
            echo "-- genPrimitiveTable.cmake: $(grep -c '^static void w_' "$dir/CairoPlugin_primitives.c") trampolines, ok"
        else
            echo "-- genPrimitiveTable.cmake: FAILED"
            ok=0
        fi
    fi
    flags="-std=gnu11 -O1 -g $asan $vmdefs $incs $vminc -DCP_PRIMITIVES_TXT=\"$prims_txt\" $GLUE_FLAGS"
    for mode in shared builtin; do
        modedefs= link=-rdynamic
        test $mode = builtin && modedefs=-DSQUEAK_BUILTIN_PLUGIN link=
        out=$dir/$mode
        mkdir -p "$out"
        if ! { $cc $flags $modedefs -c "$test_c" -o "$out/test.o" &&
               $cc $flags $modedefs -c "$SRCDIR/tests/fakevm.c" -o "$out/fakevm.o" &&
               $cc $flags $modedefs -c "$binding" -o "$out/cpCairo.o" &&
               $cxx $asan $link -o "$out/$name" "$out/test.o" "$out/fakevm.o" "$out/cpCairo.o" $engine -ldl -lm; } \
                > "$out/build.log" 2>&1; then
            cat "$out/build.log"
            echo "-- $name ($mode): FAILED (build)"
            ok=0
            continue
        fi
        echo "-- $name as a $mode plugin:"
        if ! LD_LIBRARY_PATH=$(dirname "$libcairo") "$out/$name" --libcairo "$libcairo" $(font_args "$name"); then
            ok=0
        fi
    done
    test $ok = 1 && lane_ok "$name" || lane_failed "$name"
}

lane_glue_wasm64() {
    echo "== glue-wasm64"
    wasm_flags || { lane_failed glue-wasm64; return; }
    if ! test -x "$node"; then
        lane_failed glue-wasm64 "no node ($node)"
        return
    fi
    dir=$build/glue-wasm64
    mkdir -p "$dir"
    vmlib=$wasmbuild/cmake/build/vm
    wdeps=$wasmbuild/cmake/deps
    libs="$vmlib/libpharo_cairo.a $vmlib/libpharo_pixman.a $vmlib/libpharo_png.a $vmlib/libpharo_freetype.a $vmlib/libpharo_zlib.a $wdeps/libpharo_su_engine.a $wdeps/libpharo_simdutf.a"
    for f in $libs; do
        test -f "$f" || { lane_failed glue-wasm64 "no $f"; return; }
    done
    wflags="-m64 -sMEMORY64=1 -sSUPPORT_LONGJMP=wasm -std=gnu11 -O1 -g $wdefs $wincs $incs -DCP_PRIMITIVES_TXT=\"$prims_txt\" $GLUE_FLAGS"
    ran=0 wok=1
    for test_c in "$SRCDIR"/tests/glue-*.c; do
        test -f "$test_c" || continue
        name=$(basename "$test_c" .c)
        ran=1
        if ! { "$em/emcc" $wflags -c "$test_c" -o "$dir/$name.o" &&
               "$em/emcc" $wflags -c "$SRCDIR/tests/fakevm.c" -o "$dir/fakevm.o" &&
               "$em/emcc" $wflags -c "$binding" -o "$dir/cpCairo.o" &&
               "$em/em++" -m64 -sMEMORY64=1 -sSUPPORT_LONGJMP=wasm -sNODERAWFS=1 -sALLOW_MEMORY_GROWTH=1 \
                   -sMAXIMUM_MEMORY=4GB -sSTACK_SIZE=8MB -o "$dir/$name.js" "$dir/$name.o" "$dir/fakevm.o" "$dir/cpCairo.o" $libs; } \
                > "$dir/$name.build.log" 2>&1; then
            cat "$dir/$name.build.log"
            echo "-- $name: FAILED (build)"
            wok=0
            continue
        fi
        echo "-- $name (wasm64, builtin, static cairo):"
        if ! "$node" "$dir/$name.js" $(font_args "$name"); then
            echo "-- $name: FAILED"
            wok=0
        fi
    done
    if test $ran = 0; then
        echo "-- no tests/glue-*.c"
        lane_failed glue-wasm64
        return
    fi
    test $wok = 1 && lane_ok glue-wasm64 || lane_failed glue-wasm64
}

# LeakSanitizer: what cairo's bundled dependencies keep for good (fontconfig's
# caches, FreeType's) is not the plugin's
printf 'leak:libfontconfig\nleak:libfreetype\nleak:libpixman\nleak:libcairo\n' > "$build/lsan.supp"
export ASAN_OPTIONS=${ASAN_OPTIONS:-detect_leaks=1} UBSAN_OPTIONS=${UBSAN_OPTIONS:-print_stacktrace=1}
export LSAN_OPTIONS=${LSAN_OPTIONS:-suppressions=$build/lsan.supp:print_suppressions=0}
for lane in $lanes; do
    case $lane in
    bind) lane_bind ;;
    protocheck) lane_protocheck ;;
    targets) lane_targets ;;
    symbols) lane_symbols ;;
    wasm-syntax)
        if test -z "$em" || ! test -x "$em/emcc"; then lane_failed wasm-syntax "no emcc (--emsdk)"; else lane_wasm_syntax; fi ;;
    glue-admin) lane_glue glue-admin WP-A ;;
    glue-basic) lane_glue glue-basic WP-B ;;
    glue-shape) lane_glue glue-shape WP-C ;;
    glue-text) lane_glue glue-text WP-C ;;
    glue-wasm64)
        if test -z "$em" || ! test -x "$em/emcc"; then lane_failed glue-wasm64 "no emcc (--emsdk)"; else lane_glue_wasm64; fi ;;
    esac
done

if test $failed = 0; then
    echo "run-c-tests: all lanes passed"
else
    echo "run-c-tests: FAILED"
fi
exit $failed
