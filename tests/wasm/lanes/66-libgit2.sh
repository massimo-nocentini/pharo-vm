#!/bin/sh
# 66-libgit2 - libgit2 in the WebAssembly VM, in node: Iceberg on local repositories
#
# build-wasm/node/pharo on fresh copies of the stock image (and of the
# prepared image of the world) runs tests/wasm/st/iceflow.st, which goes
# through what a user of Iceberg does with a repository of their own, with
# the image's own bindings (LGit, the library git2 of the registry, found as
# the placeholder node/libgit2.so.1.4.4): a repository, a package, two
# commits and a diff, the log, a branch, the status, and a clone of it
# through a file:// URL.
# The user of the commits is the one of a .gitconfig that the lane writes
# in a HOME of its own.
#
#   flow      every line of the golden file of the image's version,
#             tests/wasm/golden/iceflow-p<major>.txt, must be a line of what
#             it prints (a line '<key>: <id>' there stands for the short id
#             of a commit, 7 hexadecimal digits, which changes with the
#             time): every step, libgit2 1.4.4, the methods of LGit whose
#             callouts the flow compiled, the 65 of them, and the classes of
#             the callbacks it made, all 9 of them; and it must print as
#             many methods as the golden file lists, no other;
#   web       the same flow on the prepared image of the world (image/web),
#             against the same golden file;
#   adapted   the flow is run with PHARO_WASM_FFI_TRACE=1, which has the
#             width adapter of the FFI (src/emscripten/ffiAdapt.c) name each
#             function whose declaration it adapted, once, on stderr: every
#             name must be in tests/wasm/golden/adapted-66.txt, and those
#             marked required there must be named (git_tree_entrycount,
#             which LGitTree declares with an int for C's size_t); a run
#             that names none fails, since the trace would then say nothing;
#   adapt off the negative control of the adapter: with PHARO_WASM_FFI_ADAPT=0
#             the first commit must fail at the callout of
#             LGitTree>>tree_entrycount:, with exactly one line of the guard
#             of the callouts on stderr, a mismatch, and the trace must name
#             nothing;
#   unresolved the negative control of the names: a clone of
#             git://example.invalid must fail with libgit2's 'failed to
#             resolve address for example.invalid' (the VMs resolve no name:
#             src/emscripten/gitSupport.c answers EAI_FAIL to libgit2's
#             getaddrinfo), and the VM must go on;
#   hidden    the negative control of the registry: with git2 hidden from
#             the VM (PHARO_WASM_FFI_HIDE=git2), the first callout of LGit
#             must fail with a SymbolNotFoundError, without a crash or a
#             failed callout, and the VM must go on.  It runs on a copy of
#             the stock image saved without LGitLibrary in its start: the
#             stock image initializes libgit2 at each start, and the error
#             of that, with git2 hidden, ends the VM with status 1 (the
#             image's StartupUIManager defers it to an action that quits),
#             before the script runs or after it, as the image goes.
#
# Every run must exit with 0, print no line of the guard of the callouts
# ('FFI callout failed, its declaration does not match ...' or 'FFI callout
# trapped in ...') but those the control expects, nothing else on stderr but
# the lines of the trace, and write no PharoDebug.log.
#
# The golden files are the reference.  With HOST_PHARO, the flow is also
# run natively on the stock image and compared with them, as an advice only
# (a note, never a failure), and only when that VM is 10.3.9 or later: on
# an older one, Pharo 15 stops at its check of the UUIDs.  The lane says
# which reference it used.
#
# Skipped when the build has no libgit2 (WASM_LIBGIT2=OFF, the default, or
# WASM_FFI=OFF): its node VM's registry (cmake/wasm/ffi/ffiRegistry-node.c)
# has no library git2.
#
# Environment (from make wasm-check): NODE, WASM_DIR, HOST_PHARO, SRCDIR,
# TEST_DIR, and WASM_CHECK_TIMEOUT (see lib/common.sh).

SRCDIR=$(cd "${SRCDIR:-$(dirname "$0")/../../..}" && pwd)
TEST_DIR=${TEST_DIR:+$TEST_DIR/66-libgit2}
. "$SRCDIR/tests/wasm/lib/common.sh"
lane=git

if ! test -x "$WASM_DIR/node/pharo"; then
    echo "FAIL 66-libgit2: no $WASM_DIR/node/pharo (make wasm)"
    exit 1
fi
if ! grep -Eiq '^FEATURE_FFI:BOOL=(ON|TRUE|YES|Y|1)$' "$WASM_DIR/cmake/CMakeCache.txt" 2>/dev/null; then
    skip "no FFI: the build has none (WASM_FFI=OFF), so no libgit2"
fi
registry=$WASM_DIR/cmake/wasm/ffi/ffiRegistry-node.c
if ! test -f "$registry"; then
    echo "FAIL 66-libgit2: no $registry, the registry of the node VM (make wasm)"
    exit 1
fi
if ! grep -q '^extern const PharoFFILibrary pharoFFILibrary_git2;$' "$registry"; then
    skip "no libgit2: the registry of the node VM has no library git2 (WASM_LIBGIT2=OFF)"
fi
echo "66-libgit2: $WASM_DIR/node/pharo"

script=$SRCDIR/tests/wasm/st/iceflow.st
golden_dir=$SRCDIR/tests/wasm/golden
adapted_list=$golden_dir/adapted-66.txt

# The lines of the guard of the callouts, of the trace of the adapter, and
# what the runtime of a debug build says of itself, as in lane 65: vm()
# keeps the guard's in guard.err and the trace's in trace.err, out of the
# stderr the checks see (the message of what threw may name an error of the
# JavaScript runtime, see engine_errors in lib/common.sh), and leaves the
# runtime's out; the rest of stderr goes on.  A line of the trace is
# 'FFI callout adapted: <function> ...', the name of the function first.
guard='^FFI callout (failed, its declaration does not match|trapped in) '
trace='^FFI callout adapted: '
runtime='^(Heap resize call from [0-9]+ to [0-9]+ took .* Success: true|program exited \(with status: [0-9]+\), but keepRuntimeAlive\(\) is set)'
# (a HOME of the check's own, whose .gitconfig names the user of the
# commits; XDG_CONFIG_HOME in it too, so that libgit2 reads no
# configuration of the user who runs the lane)
home() {
    mkdir -p home &&
	printf '[user]\n\tname = Wasm Test\n\temail = wasm@example.org\n' >home/.gitconfig
}
vm() {
    if HOME=$(pwd)/home XDG_CONFIG_HOME=$(pwd)/home/.config \
	limit "$timeout" "$1" --headless Pharo.image --no-default-preferences st --quit "$script" 2>vm.err
    then vm_status=0; else vm_status=$?; fi
    grep -E "$guard" vm.err >guard.err
    grep -E "$trace" vm.err >trace.err
    grep -Ev "$guard|$trace|$runtime" vm.err >&2
    return $vm_status
}
# (each run has the trace on: its lines are the evidence of the adapted
# check, and a control must show that it says nothing)
flow() ( PHARO_WASM_FFI_TRACE=1; export PHARO_WASM_FFI_TRACE; vm "$WASM_DIR/node/pharo" )
adapt_off() (
    PHARO_WASM_FFI_TRACE=1 PHARO_WASM_FFI_ADAPT=0 WASM_ICEFLOW_STEPS='create repo,make class,add package,commit 1'
    export PHARO_WASM_FFI_TRACE PHARO_WASM_FFI_ADAPT WASM_ICEFLOW_STEPS
    vm "$WASM_DIR/node/pharo"
)
unresolved() (
    WASM_ICEFLOW_STEPS='libgit2 version,unresolved clone,after'
    export WASM_ICEFLOW_STEPS
    vm "$WASM_DIR/node/pharo"
)
hidden() (
    PHARO_WASM_FFI_HIDE=git2 WASM_ICEFLOW_STEPS='libgit2 version,create repo,after'
    export PHARO_WASM_FFI_HIDE WASM_ICEFLOW_STEPS
    vm "$WASM_DIR/node/pharo"
)

# value KEY: the value of the line 'KEY: ...' of smoke.out
value() {
    sed -n "s/^$1: //p" smoke.out | head -n 1
}
# adapted: the functions that trace.err names, sorted, one a line
adapted() {
    sed -n 's/^FFI callout adapted: \([A-Za-z_][A-Za-z0-9_]*\).*/\1/p' trace.err | sort -u
}

# exited NAME: fails NAME and answers false after a run that did not exit
# with 0, printed anything on stderr but the lines of the guard and of the
# trace, or wrote PharoDebug.log
exited() {
    if test "$status" -ne 0; then
	fail "$1" "exit status $status"
    elif test -s smoke.err; then
	fail "$1" "stderr is not empty: $(head -n 1 smoke.err | cut -c 1-200)"
    elif test -s PharoDebug.log; then
	fail "$1" "it wrote PharoDebug.log: $(grep -m 1 . PharoDebug.log | cut -c 1-200)"
    else
	return 0
    fi
    return 1
}
# quiet NAME: exited NAME, and no line of the guard
quiet() {
    exited "$1" || return 1
    if test -s guard.err; then
	fail "$1" "$(grep -c '' guard.err) failed callouts: $(head -n 1 guard.err)"
	return 1
    fi
    return 0
}

# differ OUT GOLDEN: writes to differ.txt each line of GOLDEN that is not a
# line of OUT ('#' lines and blank lines are comments; a value '<id>' is a
# short id of a commit), and each 'ffi method' line of OUT that GOLDEN does
# not have; answers false when it wrote any
differ() {
    : >differ.txt
    grep -v '^#' "$2" | grep -v '^[ 	]*$' | while IFS= read -r line; do
	key=${line%%: *}
	got=$(sed -n "s/^$key: //p" "$1" | head -n 1)
	case $line in
	    *': <id>')
		echo "$got" | grep -Eqx '[0-9a-f]{7}' ||
		    echo "$key: expected the short id of a commit, got '$(echo "$got" | cut -c 1-200)'" >>differ.txt ;;
	    *)
		grep -qxF "$line" "$1" ||
		    echo "$key: expected '${line#*: }', got '$(echo "$got" | cut -c 1-200)'" >>differ.txt ;;
	esac
    done
    grep '^ffi method: ' "$1" | while IFS= read -r line; do
	grep -qxF "$line" "$2" || echo "$line, which the golden file does not list" >>differ.txt
    done
    ! test -s differ.txt
}

# golden NAME: smoke.out against the golden file of its major version
golden() {
    major=$(value major)
    golden=$golden_dir/iceflow-p$major.txt
    if ! test -f "$golden"; then
	fail "$1" "no golden file $golden for the Pharo of the image (major: '$major')"
	return 1
    fi
    if ! differ smoke.out "$golden"; then
	fail "$1" "differs from ${golden#$SRCDIR/}: $(tr '\n' ';' <differ.txt | cut -c 1-1500)"
	return 1
    fi
    return 0
}

# the times of the steps of the flow, for the note of a check
step_times() {
    for step in 'create repo' 'commit 1' modify 'commit 2' log 'file clone'; do
	printf '%s %s; ' "$step" "$(value "ms $step")"
    done
}

# allowed NAME: the functions of trace.err against adapted-66.txt, whose
# lines are '<function>' or '<function> required'
allowed() {
    if ! test -f "$adapted_list"; then
	fail "$1" "no $adapted_list"
	return 1
    fi
    names=$(adapted)
    known=$(grep -v '^#' "$adapted_list" | awk 'NF { print $1 }')
    required=$(grep -v '^#' "$adapted_list" | awk '$2 == "required" { print $1 }')
    unknown=
    for name in $names; do
	echo "$known" | grep -qxF "$name" || unknown="$unknown $name"
    done
    absent=
    for name in $required; do
	echo "$names" | grep -qxF "$name" || absent="$absent $name"
    done
    if test -z "$names"; then
	fail "$1" "the trace (PHARO_WASM_FFI_TRACE=1) named no adapted function: it says nothing, or the adapter is off"
    elif test -n "$unknown"; then
	fail "$1" "adapted functions that ${adapted_list#$SRCDIR/} does not list:$unknown (all: $(echo $names))"
    elif test -n "$absent"; then
	fail "$1" "required adapted functions the trace did not name:$absent (it named $(echo $names))"
    else
	ok "$1" "$(echo "$names" | grep -c .) functions: $(echo $names)"
    fi
}

stock=$WASM_DIR/image/stock
fresh flow
home
if run flow flow; then
    quiet flow && golden flow &&
	ok flow "libgit2 $(value 'libgit2 version'), $(value 'ffi methods') FFI methods, $(value 'callback classes' | wc -w) callback classes, 0 failed callouts, golden iceflow-p$(value major).txt; ms: $(step_times)"
    flow_major=$(value major)
    # (the trace of the same run)
    checks=$((checks + 1))
    allowed adapted
fi

web=$WASM_DIR/image/web
if ls "$web"/*.image >/dev/null 2>&1; then
    fresh web "$web"
    home
    run web flow && quiet web && golden web &&
	ok web "$(value 'ffi methods') FFI methods, $(value 'callback classes' | wc -w) callback classes, golden iceflow-p$(value major).txt; ms: $(step_times)"
else
    skip_check web "no image in $web (WASM_WORLD=OFF, or no WASM_HOST_PHARO)"
fi

fresh adapt-off
home
run 'adapt off' adapt_off && exited 'adapt off' && {
    guards=$(grep -c '' guard.err)
    commit=$(value 'commit 1')
    case $commit in
	'error '*'(in LGitTree>>tree_entrycount:)') ;;
	*) commit= ;;
    esac
    if test -z "$commit"; then
	fail 'adapt off' "with the adapter off, the commit did not fail at LGitTree>>tree_entrycount:: '$(value 'commit 1' | cut -c 1-200)'"
    elif test "$guards" -ne 1 || ! grep -q '^FFI callout failed, its declaration does not match' guard.err; then
	fail 'adapt off' "expected 1 failed callout on stderr, a mismatch, got $guards: $(head -n 3 guard.err | tr '\n' ';')"
    elif test -s trace.err; then
	fail 'adapt off' "with the adapter off, the trace named $(adapted | tr '\n' ' ')"
    else
	ok 'adapt off' "1 failed callout: $(cut -c 1-90 guard.err); commit 1: $(echo "$commit" | sed 's/.*(in /(in /')"
    fi
}

fresh unresolved
home
run unresolved unresolved && quiet unresolved && {
    clone=$(value 'unresolved clone')
    case $clone in
	'error '*'failed to resolve address for example.invalid'*)
	    if test "x$(value after)" = x7; then
		ok unresolved "$(echo "$clone" | cut -c 1-120)"
	    else
		fail unresolved "the VM did not go on after the clone: after is '$(value after)'"
	    fi ;;
	*) fail unresolved "the clone of git://example.invalid did not fail to resolve the address: '$(echo "$clone" | cut -c 1-200)'" ;;
    esac
}

# (the copy of the hidden control leaves LGitLibrary out of the start of
# the image: LGitLibrary class>>startUp: initializes libgit2 at each start,
# and passes on what that raised, which StartupUIManager defers to an action
# that quits with status 1, before the script or after it, as it goes; what
# the runtime of a debug build says of itself is left out, as in vm())
unregistered() {
    if limit "$timeout" "$WASM_DIR/node/pharo" --headless Pharo.image --no-default-preferences \
	eval --save 'SessionManager default unregisterClassNamed: #LGitLibrary. #unregistered' >unregister.raw 2>&1
    then save_status=0; else save_status=$?; fi
    grep -Ev "$runtime" unregister.raw >unregister.out
    test $save_status = 0 && test "x$(cat unregister.out)" = 'x#unregistered'
}
fresh hidden
home
if ! unregistered; then
    checks=$((checks + 1))
    fail hidden "could not save the copy without LGitLibrary at its start: $(head -n 1 unregister.out | cut -c 1-200)"
else
    run hidden hidden && quiet hidden && {
	version=$(value 'libgit2 version')
	case $version in
	    'error SymbolNotFoundError'*)
		if test "x$(value after)" = x7; then
		    ok hidden "$(echo "$version" | cut -c 1-100)"
		else
		    fail hidden "the VM did not go on: after is '$(value after)'"
		fi ;;
	    *) fail hidden "with git2 hidden, the version of libgit2 did not fail with a SymbolNotFoundError: '$(echo "$version" | cut -c 1-200)'" ;;
	esac
    }
fi

# The advice of a native VM: the same flow on the stock image, on a VM that
# runs both Pharo 12 and Pharo 15 (10.3.9 or later)
host_version=
if test -n "$HOST_PHARO" && test -x "$HOST_PHARO"; then
    host_version=$("$HOST_PHARO" --version 2>/dev/null | sed -n '1s/^Pharo v\{0,1\}\([0-9][0-9]*\.[0-9][0-9]*\.[0-9][0-9]*\).*/\1/p')
fi
if test -z "$HOST_PHARO" || ! test -x "$HOST_PHARO"; then
    echo "note [$lane] no HOST_PHARO: the reference is the golden file, iceflow.st is not compared with a native VM"
elif test -z "$host_version"; then
    echo "note [$lane] HOST_PHARO says no version: the reference is the golden file, iceflow.st is not compared with a native VM"
elif ! echo "$host_version" | awk -F . '{ exit !($1 * 1000000 + $2 * 1000 + $3 >= 10003009) }'; then
    echo "note [$lane] HOST_PHARO is VM $host_version, older than 10.3.9 (where Pharo 15 stops at its check of the UUIDs): the reference is the golden file, iceflow.st is not compared with a native VM"
elif test -z "$flow_major"; then
    echo "note [$lane] no flow: iceflow.st is not compared with a native VM"
else
    retire
    native=$TEST_DIR/$lane-native
    rm -rf "$native"
    mkdir -p "$native" && copy_image "$stock" "$native" && (
	cd "$native" && home && vm "$HOST_PHARO"
    ) >"$native/native.out" 2>"$native/native.err"
    native_status=$?
    native_golden=$golden_dir/iceflow-p$flow_major.txt
    # (libgit2 and the file it is found in are the native VM's own: only the
    # steps and what LGit called are compared)
    grep -Ev '^(library|libgit2 version): ' "$native_golden" >"$native/golden.txt"
    native_libgit2=$(sed -n 's/^libgit2 version: //p' "$native/native.out")
    if test "$native_status" -ne 0 || ! grep -q '^ffi methods: ' "$native/native.out"; then
	echo "note [$lane] HOST_PHARO, VM $host_version: iceflow.st did not run natively (status $native_status; kept $native)"
    elif case $native_libgit2 in error*) true ;; *) false ;; esac; then
	echo "note [$lane] HOST_PHARO, VM $host_version: the image finds no libgit2 there, iceflow.st is not compared with it: $(echo "$native_libgit2" | cut -c 1-120)"
	rm -rf "$native"
    elif (cd "$native" && differ native.out golden.txt); then
	echo "note [$lane] HOST_PHARO, VM $host_version, libgit2 $native_libgit2: the same steps as the golden iceflow-p$flow_major.txt, which is the reference"
	rm -rf "$native"
    else
	echo "note [$lane] HOST_PHARO, VM $host_version: other steps than the golden iceflow-p$flow_major.txt, which is the reference:" \
	    "$(tr '\n' ';' <"$native/differ.txt" | cut -c 1-400) (kept $native)"
    fi
fi

finish 66-libgit2
