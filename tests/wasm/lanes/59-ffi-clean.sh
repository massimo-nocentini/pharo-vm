#!/bin/sh
# 59-ffi-clean - the FFI of the images themselves, in node, with no failure
#
# build-wasm/node/pharo on fresh copies of the stock image and of the
# prepared image of the world (image/web, when the build has one):
#
#   boot      a boot that evaluates 3 + 4;
#   session   tests/wasm/st/ffi-session.st, the image's own callouts into
#             the C library (the environment, getpid, strerror, a qsort
#             with a callback), which saves the image, then the same again
#             on the saved image, after the reload;
#   control   the negative control, on that image: one callout declared
#             double for strlen, which must fail as a primitive with
#             exactly one line of the guard of the callouts on stderr,
#             which shows that the lines would be seen;
#   coverage  packaging/emscripten/tools/ffi-symbols.st lists the functions
#             the image binds, test packages included, with the library
#             of the registry each one is bound in: every function of a
#             library of the node VM's registry
#             (cmake/wasm/ffi/ffiRegistry-node.c of the build) must be a
#             row of its table (cmake/wasm/ffi/ffi_<library>.c), or be
#             exempt in its cmake/emscripten/ffi/symbols/<library>.txt
#             (a '# unavailable:' section, or a '# unavailable unless
#             <VARIABLE>:' one whose variable is off in the CMake cache).
#
# The boots and the session must print no line of the guard ('FFI callout
# failed, its declaration does not match ...' or 'FFI callout trapped in
# ...'), and no unhandled error or debugger: their stderr must be empty and
# they must write no PharoDebug.log, which is what a boot of either image
# does today, with or without the FFI.  Skipped when the build has no FFI
# (WASM_FFI=OFF).
#
# Environment (from make wasm-check): NODE, WASM_DIR, SRCDIR, TEST_DIR, and
# WASM_CHECK_TIMEOUT (see lib/common.sh).

SRCDIR=$(cd "${SRCDIR:-$(dirname "$0")/../../..}" && pwd)
TEST_DIR=${TEST_DIR:+$TEST_DIR/59-ffi-clean}
. "$SRCDIR/tests/wasm/lib/common.sh"
lane=ffi-clean

if ! test -x "$WASM_DIR/node/pharo"; then
    echo "FAIL 59-ffi-clean: no $WASM_DIR/node/pharo (make wasm)"
    exit 1
fi
cache=$WASM_DIR/cmake/CMakeCache.txt
if ! grep -Eiq '^FEATURE_FFI:BOOL=(ON|TRUE|YES|Y|1)$' "$cache" 2>/dev/null; then
    skip "no FFI: the build has none (WASM_FFI=OFF)"
fi
ffi_dir=$WASM_DIR/cmake/wasm/ffi
if ! test -f "$ffi_dir/ffiRegistry-node.c"; then
    echo "FAIL 59-ffi-clean: no $ffi_dir/ffiRegistry-node.c, the registry of the node VM (make wasm)"
    exit 1
fi
echo "59-ffi-clean: $WASM_DIR/node/pharo"

session=$SRCDIR/tests/wasm/st/ffi-session.st
symbols=$SRCDIR/packaging/emscripten/tools/ffi-symbols.st

# The lines of the guard of the callouts.  vm() keeps them in guard.err,
# out of the stderr the checks see (the message of what threw may name an
# error of the JavaScript runtime, see engine_errors in lib/common.sh), and
# leaves out what the runtime of a debug build (WASM_DEBUG=1, with
# Emscripten's ASSERTIONS) says of itself, the growth of the memory and the
# exit while the runtime is kept alive; the rest of stderr goes on.
guard='^FFI callout (failed, its declaration does not match|trapped in) '
runtime='^(Heap resize call from [0-9]+ to [0-9]+ took .* Success: true|program exited \(with status: [0-9]+\), but keepRuntimeAlive\(\) is set)'
vm() {
    if limit "$timeout" "$WASM_DIR/node/pharo" --headless Pharo.image --no-default-preferences "$@" 2>vm.err
    then vm_status=0; else vm_status=$?; fi
    grep -E "$guard" vm.err >guard.err
    grep -Ev "$guard|$runtime" vm.err >&2
    return $vm_status
}
control() ( WASM_FFI_CONTROL=1; export WASM_FFI_CONTROL; vm st --quit "$session" )
listing() ( PHARO_FFI_SYMBOLS=$check_dir/symbols.txt; export PHARO_FFI_SYMBOLS; vm st --quit "$symbols" )

# clean NAME EXPECTED: after run NAME, stdout must be EXPECTED (a line of
# it, for a session, whose checks print a line each), with status 0, no
# line of the guard, nothing else on stderr and no PharoDebug.log
clean() {
    if test "$status" -ne 0; then
	fail "$1" "exit status $status"
    elif test -s guard.err; then
	fail "$1" "$(grep -c '' guard.err) failed callouts: $(head -n 1 guard.err)"
    elif test -s smoke.err; then
	fail "$1" "stderr is not empty: $(head -n 1 smoke.err | cut -c 1-200)"
    elif test -s PharoDebug.log; then
	fail "$1" "it wrote PharoDebug.log: $(grep -m 1 . PharoDebug.log | cut -c 1-200)"
    elif grep -q '^FAIL ' smoke.out; then
	fail "$1" "$(grep '^FAIL ' smoke.out | tr '\n' ';')"
    elif ! grep -qx "$2" smoke.out; then
	fail "$1" "expected '$2', got '$(tail -n 1 smoke.out)'"
    else
	ok "$1" "$2"
    fi
}

# covered NAME: the functions of symbols.txt ('<library> TAB <function> TAB
# <method>') against the tables of the registry of the node VM; fails with
# status 1 when functions are missing (missing.txt), and 2 when it failed
# NAME itself
covered() {
    libraries=$(sed -n 's/^extern const PharoFFILibrary pharoFFILibrary_\([A-Za-z0-9_]*\);$/\1/p' \
	"$ffi_dir/ffiRegistry-node.c")
    if test -z "$libraries"; then
	fail "$1" "no library in $ffi_dir/ffiRegistry-node.c"
	return 2
    fi
    : >table.txt
    : >exempt.txt
    for library in $libraries; do
	if ! test -f "$ffi_dir/ffi_$library.c"; then
	    fail "$1" "no table $ffi_dir/ffi_$library.c"
	    return 2
	fi
	# the rows: {"<name>", (void *)&<name>...}
	sed -n 's/^[ 	]*{"\([^"]*\)", *(void *\*).*/\1/p' "$ffi_dir/ffi_$library.c" |
	    sed "s/^/$library	/" >>table.txt
	list=$SRCDIR/cmake/emscripten/ffi/symbols/$library.txt
	if test -f "$list"; then
	    # the names of the sections that exempt them: '# unavailable:'
	    # always, '# unavailable unless VAR:' when VAR is off
	    awk -v library="$library" -v cache="$cache" '
		BEGIN {
		    while ((getline line < cache) > 0)
			if (match(line, /^[A-Za-z0-9_]+(:[A-Z]+)?=/)) {
			    name = line; sub(/[:=].*/, "", name)
			    value = line; sub(/^[^=]*=/, "", value)
			    on[name] = toupper(value) ~ /^(ON|TRUE|YES|Y|1)$/
			}
		    exempt = 0
		}
		/^#[ \t]*unavailable unless [A-Za-z0-9_]+:/ {
		    v = $0; sub(/^#[ \t]*unavailable unless /, "", v); sub(/:.*/, "", v)
		    exempt = !on[v]; next
		}
		/^#[ \t]*unavailable:/ { exempt = 1; next }
		/^#/ || NF == 0 { next }
		exempt { print library "\t" $1 }' "$list" >>exempt.txt
	fi
    done
    rm -f missing.txt
    awk -F '	' -v libraries="$(echo $libraries)" '
	BEGIN { n = split(libraries, names, " "); for (i = 1; i <= n; i++) registered[names[i]] = 1 }
	FILENAME == "table.txt" { row[$1 FS $2] = 1; next }
	FILENAME == "exempt.txt" { exempt[$1 FS $2] = 1; next }
	NF >= 2 {
	    key = $1 FS $2
	    if (!($1 in registered)) { if (!seen[key]++) other[$1] += 1; next }
	    if (seen[key]++) next
	    if (key in row) covered[$1] += 1
	    else if (key in exempt) exempted[$1] += 1
	    else { missing += 1; print $1 " " $2 " (" $3 ")" > "missing.txt" }
	}
	END {
	    line = ""
	    for (i = 1; i <= n; i++) {
		l = names[i]
		line = line (line == "" ? "" : ", ") l " " covered[l] + 0
		if (exempted[l]) line = line " (and " exempted[l] " exempt)"
	    }
	    outside = ""
	    for (l in other) outside = outside (outside == "" ? "" : ", ") l " " other[l]
	    print "functions in the registry: " line > "coverage.txt"
	    print "outside it: " (outside == "" ? "none" : outside) > "coverage.txt"
	    exit (missing > 0 ? 1 : 0)
	}' table.txt exempt.txt symbols.txt
}

for image in stock web; do
    from=$WASM_DIR/image/$image
    if ! ls "$from"/*.image >/dev/null 2>&1; then
	skip_check "boot $image" "no image in $from (WASM_WORLD=OFF, or no WASM_HOST_PHARO)"
	continue
    fi

    fresh "boot-$image" "$from"
    run "boot $image" vm eval '3 + 4' && clean "boot $image" 7

    fresh "session-$image" "$from"
    run "session $image" vm st --quit "$session" &&
	clean "session $image" 'ffi-session first: 6 checks, 0 failed'
    if test "$check_failed" -eq 0; then
	run "reload $image" vm st --quit "$session" &&
	    clean "reload $image" 'ffi-session reload: 7 checks, 0 failed'
    fi
    if test "$check_failed" -eq 0; then
	run "control $image" control && {
	    guards=$(grep -c '' guard.err)
	    if test "$status" -ne 0; then
		fail "control $image" "exit status $status"
	    elif ! grep -qx 'ffi-session control: #failed' smoke.out; then
		fail "control $image" "expected 'ffi-session control: #failed', got '$(tail -n 1 smoke.out)'"
	    elif test "$guards" -ne 1 || ! grep -q '^FFI callout failed, its declaration does not match' guard.err; then
		fail "control $image" "expected 1 failed callout on stderr, a mismatch, got $guards"
	    elif test -s smoke.err; then
		fail "control $image" "stderr has more: $(head -n 1 smoke.err | cut -c 1-200)"
	    else
		ok "control $image" "1 failed callout: $(cut -c 1-80 guard.err)"
	    fi
	}
    fi

    fresh "coverage-$image" "$from"
    run "coverage $image" listing && {
	if test "$status" -ne 0; then
	    fail "coverage $image" "ffi-symbols.st: exit status $status"
	elif test -s guard.err; then
	    fail "coverage $image" "ffi-symbols.st: $(head -n 1 guard.err)"
	elif ! test -s symbols.txt; then
	    fail "coverage $image" "ffi-symbols.st listed no function"
	else
	    covered "coverage $image"
	    case $?,$(test -s missing.txt && echo missing) in
		0,) ok "coverage $image" "$(sed -n 1p coverage.txt); $(sed -n 2p coverage.txt)" ;;
		2,*) ;;
		*,missing) fail "coverage $image" "$(grep -c '' missing.txt) functions the image binds are neither in the registry nor exempt: $(head -n 5 missing.txt | tr '\n' ';')" ;;
		*) fail "coverage $image" "the functions could not be compared with the tables" ;;
	    esac
	fi
    }
done

finish 59-ffi-clean
