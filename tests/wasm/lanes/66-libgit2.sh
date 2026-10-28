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
# in a HOME of its own (but for the identity checks, whose HOME is empty).
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
#   identity <image>
#             the first commit of a user that git does not know yet, as the
#             browser has it (its HOME has no .gitconfig at first):
#             tests/wasm/st/git-identity.st, with a HOME of its own that is
#             empty, goes through Iceberg's dialog of the missing
#             properties (IceTipGitUsernameAndEmailDialogPresenter>>#accept,
#             not opened, 'Save properties as global?' checked), then a full
#             garbage collection, whose finalization frees the LGitConfig
#             that the dialog made, then two commits more; on each world
#             image of the site (image/web, and image/web-<id> of an extra
#             image), every line of tests/wasm/golden/git-identity.txt must
#             be a line of what it prints: LGitConfig frees with
#             config_free: (OSWebDriver class>>#setUpLibGit set the image
#             up so, or its Pharo has it), whose callout is
#             git_config_free, and a second setUpLibGit compiles nothing;
#             the dialog was asked once, the user went into
#             HOME/.gitconfig, and the three commits are there, by that
#             user;
#   control <image>
#             the same on each stock image of the site (image/stock, and
#             image/stock-<id>), which nothing set up: when its LGitConfig
#             frees with object_free: (Pharo 13 and later), the steps up to
#             the dialog must work and a commit must then fail (the
#             finalization may come during the first one already, at a
#             garbage collection of its own), which shows that the identity
#             check sees the bug that the set up removes; when it frees
#             with config_free: (Pharo 12), the stock image must pass as a
#             world image does;
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
#             before the script runs or after it, as the image goes;
#   redirect  the smart-HTTP transport of the VM (src/emscripten/gitSupport.c,
#             WASM_LIBGIT2_HTTP), which the node VM makes with curl:
#             tests/wasm/st/git-http-clone.st clones with Iceberg from
#             tests/wasm/lib/git-http.mjs, a git server on 127.0.0.1 (`git
#             http-backend') in its redirect mode, the repository r named
#             without its .git, as GitLab and Codeberg take it: the clone must
#             check out master, with its one commit, and the server must have
#             got the GET of r/info/refs (which it redirects to r.git) and
#             then the POST of the pack to r.git/git-upload-pack, never to
#             r/git-upload-pack (which it refuses with 422): the transport
#             takes the redirect of the refs for the next requests;
#   missing   in the same run, a clone of a repository that is not there must
#             fail with the transport's 'git http: no git repository at
#             <url> (404)', after a single request (GIT_ERROR_HTTP: Iceberg
#             does not retry it), and the VM must go on.
#             Both run without the PHARO_WASM_GIT_PROXY of the environment
#             (the requests never go to a proxy), both fail when the server
#             does not start or the run of the VM fails, and both are skipped
#             without the transport (WASM_LIBGIT2_HTTP=OFF), curl, or git
#             with `git http-backend'.
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
# (an empty one, for the identity checks)
empty_home() {
    rm -rf home && mkdir home
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
identity() (
    script=$SRCDIR/tests/wasm/st/git-identity.st
    vm "$WASM_DIR/node/pharo"
)
# (the clones over HTTP, from the git server whose URL is $git_url, never
# through a proxy of the environment of the user who runs the lane, which
# the transport would take: the requests of 127.0.0.1 would go there)
http_clone() (
    unset PHARO_WASM_GIT_PROXY
    script=$SRCDIR/tests/wasm/st/git-http-clone.st
    WASM_GIT_HTTP_REMOTE=${git_url}r WASM_GIT_HTTP_MISSING=${git_url}nope
    export WASM_GIT_HTTP_REMOTE WASM_GIT_HTTP_MISSING
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

# The identity checks, on the world images, then the controls on the stock
# ones; <image> is the name of the directory of the image (web, web-p13,
# stock, ...)
identity_golden=$golden_dir/git-identity.txt
worlds=0
for dir in "$WASM_DIR/image/web" "$WASM_DIR"/image/web-*; do
    ls "$dir"/*.image >/dev/null 2>&1 || continue
    image=${dir##*/}
    worlds=$((worlds + 1))
    fresh "identity-$image" "$dir"
    empty_home
    run "identity $image" identity && quiet "identity $image" && {
	if differ smoke.out "$identity_golden"; then
	    ok "identity $image" "Pharo $(value major), frees with $(value 'free calls'), $(value dialogs) dialog, commits $(value 'commit 1') $(value 'commit 2') $(value 'commit 3') by $(value gitconfig) after the finalization"
	else
	    fail "identity $image" "differs from ${identity_golden#$SRCDIR/}: $(tr '\n' ';' <differ.txt | cut -c 1-1500)"
	fi
    }
done
if test "$worlds" -eq 0; then
    skip_check identity "no image in $WASM_DIR/image/web (WASM_WORLD=OFF, or no WASM_HOST_PHARO)"
fi
for dir in "$WASM_DIR/image/stock" "$WASM_DIR"/image/stock-*; do
    ls "$dir"/*.image >/dev/null 2>&1 || continue
    image=${dir##*/}
    fresh "control-$image" "$dir"
    empty_home
    run "control $image" identity && exited "control $image" && {
	free=$(value 'free function')
	errors=$(value errors)
	if test "x$(value 'add package')" != 'x#(#WasmIdentityDemo)' || test "x$(value 'dialogs 1')" != x1; then
	    fail "control $image" "the steps before the first commit did not run: add package '$(value 'add package' | cut -c 1-200)', dialogs '$(value 'dialogs 1' | cut -c 1-100)'"
	elif test "x$free" = 'xobject_free:'; then
	    case $errors in
		*"'commit 1'"*|*"'signature'"*|*"'commit 2'"*|*"'commit 3'"*)
		    ok "control $image" "Pharo $(value major), LGitConfig frees with #object_free:, so $errors failed, with $(grep -c '' guard.err) failed callouts" ;;
		*) fail "control $image" "LGitConfig frees with object_free:, but no commit failed (errors: '$(echo "$errors" | cut -c 1-200)'): the identity check would not see the bug" ;;
	    esac
	elif test "x$free" = 'xconfig_free:'; then
	    if test -s guard.err; then
		fail "control $image" "LGitConfig frees with config_free:, but $(grep -c '' guard.err) callouts failed: $(head -n 1 guard.err)"
	    elif ! differ smoke.out "$identity_golden"; then
		fail "control $image" "LGitConfig frees with config_free:, but the flow differs from ${identity_golden#$SRCDIR/}: $(tr '\n' ';' <differ.txt | cut -c 1-1000)"
	    else
		ok "control $image" "Pharo $(value major), LGitConfig frees with #config_free: already, and the flow passes"
	    fi
	else
	    fail "control $image" "LGitConfig frees with '$(echo "$free" | cut -c 1-200)'"
	fi
    }
done

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

# The smart-HTTP transport: a git server of the check's own (git-http.mjs,
# its log in server.log, a line '<server> <method> <url> <status>' a
# request), stopped at the end of the check, or of the lane
server_pid=
stop_server() {
    if test -n "$server_pid"; then
	kill "$server_pid" 2>/dev/null
	wait "$server_pid" 2>/dev/null
	server_pid=
    fi
}
trap stop_server EXIT
# start_server: starts it in the current directory, with the repository r,
# and sets git_url; false when it did not start (see server.err)
start_server() {
    "$NODE" "$SRCDIR/tests/wasm/lib/git-http.mjs" repos --redirect --seed r --log server.log \
	>server.out 2>server.err </dev/null &
    server_pid=$!
    waited=0
    while ! grep -q '^CORS proxy ' server.out 2>/dev/null; do
	if ! kill -0 "$server_pid" 2>/dev/null || test "$waited" -ge 300; then
	    stop_server
	    return 1
	fi
	waited=$((waited + 1))
	sleep 0.1
    done
    git_url=$(sed -n 's/^git server \([^ ]*\).*/\1/p' server.out)
    test -n "$git_url"
}
# (whether node finds git with `git http-backend', as the server needs; the
# module is named in the environment, since one named by process.argv[1]
# would take itself for the program and serve)
has_git() {
    GIT_HTTP_MJS=$SRCDIR/tests/wasm/lib/git-http.mjs "$NODE" -e 'import(require("url").pathToFileURL(process.env.GIT_HTTP_MJS).href)
	.then(m => process.exit(m.hasGit() ? 0 : 1), () => process.exit(1))'
}
if ! grep -Eiq '^WASM_LIBGIT2_HTTP:BOOL=(ON|TRUE|YES|Y|1)$' "$WASM_DIR/cmake/CMakeCache.txt" 2>/dev/null; then
    skip_check redirect "no smart-HTTP transport (WASM_LIBGIT2_HTTP=OFF)"
    skip_check missing "no smart-HTTP transport (WASM_LIBGIT2_HTTP=OFF)"
elif ! command -v curl >/dev/null 2>&1; then
    skip_check redirect "no curl, which the node VM makes its HTTP requests with"
    skip_check missing "no curl, which the node VM makes its HTTP requests with"
elif ! has_git; then
    skip_check redirect "no git, or no git http-backend, to serve the repositories"
    skip_check missing "no git, or no git http-backend, to serve the repositories"
else
    fresh http
    home
    mkdir -p repos
    # (one run of the VM makes both clones: when the server or the run
    # fails, missing fails too, so that the lane counts both checks)
    if ! start_server; then
	checks=$((checks + 2))
	fail redirect "the git server did not start: $(head -n 1 server.err | cut -c 1-200)"
	fail missing "the git server did not start"
    elif ! { run redirect http_clone && quiet redirect; }; then
	checks=$((checks + 1))
	fail missing "the run of the clones failed (see redirect)"
    else
	stop_server
	# (the requests of r, and of nope, in the order the server answered them)
	grep -E '^git [A-Z]+ /r(\.git)?/' server.log >r.log
	grep -E '^git [A-Z]+ /nope(\.git)?/' server.log >nope.log
	refs_at=$(grep -n -m 1 '^git GET /r/info/refs?service=git-upload-pack 301$' r.log | cut -d : -f 1)
	pack_at=$(grep -n -m 1 '^git POST /r\.git/git-upload-pack 200$' r.log | cut -d : -f 1)
	if test "x$(value 'redirect clone')" != xmaster || test "x$(value 'redirect log')" != xfirst; then
	    fail redirect "the clone of ${git_url}r: '$(value 'redirect clone' | cut -c 1-200)', its log '$(value 'redirect log' | cut -c 1-100)'"
	elif test -z "$refs_at" || test -z "$pack_at" || test "$pack_at" -lt "$refs_at"; then
	    fail redirect "the server did not get the GET of r/info/refs, then the POST of r.git/git-upload-pack: $(tr '\n' ';' <r.log | cut -c 1-400)"
	elif grep -q '^git POST /r/' r.log; then
	    fail redirect "the transport did not take the redirect for the pack: $(tr '\n' ';' <r.log | cut -c 1-400)"
	else
	    ok redirect "$(tr '\n' ';' <r.log | cut -c 1-300)"
	fi
	checks=$((checks + 1))
	missing=$(value 'missing clone')
	case $missing in
	    "error "*"git http: no git repository at ${git_url}nope/info/refs (404)")
		if test "$(grep -c '' nope.log)" -ne 1; then
		    fail missing "expected a single request of nope, got: $(tr '\n' ';' <nope.log | cut -c 1-300)"
		elif test "x$(value after)" != x7; then
		    fail missing "the VM did not go on after the clone: after is '$(value after)'"
		else
		    ok missing "$(echo "$missing" | cut -c 1-120)"
		fi ;;
	    *) fail missing "the clone of ${git_url}nope did not fail with the 404 of the transport: '$(echo "$missing" | cut -c 1-200)'" ;;
	esac
    fi
    stop_server
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
