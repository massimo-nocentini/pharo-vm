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
#             image), with packaging/emscripten/st/web-libgit.st in st/
#             next to it, as the page has it, every line of
#             tests/wasm/golden/git-identity.txt must be a line of what it
#             prints: LGitConfig frees with config_free:
#             (OSWebDriver class>>#setUpLibGit, which evaluates
#             web-libgit.st, set the image up so at build time, or its
#             Pharo has it), whose callout is git_config_free, and a second
#             setUpLibGit compiles nothing; the dialog was asked once, the
#             user went into HOME/.gitconfig, and the three commits are
#             there, by that user; LGitConfig>>#globalConfigurationPath
#             answers nil before the dialog and HOME/.gitconfig after it
#             (web-libgit.st gives git_config_find_global the git_buf, an
#             LGitBuf, that it fills, where LibGit-Core gives it a String,
#             into which libgit2 writes the bytes of the git_buf), and it
#             and LGitConfig>>#config_find_global_path: are extensions of
#             OSWindow-Web that web-libgit.st compiled, logged, the former
#             freeing its buffer in an ensure: ('global config methods:
#             OSWindow-Web/*OSWindow-Web OSWindow-Web/*OSWindow-Web marker
#             true logged true frees true'); IceMergeResolutionRequest's
#             default action is the one of web-libgit.st, which runs
#             Iceberg's own in a world, and the second set up compiles
#             neither again (the web-libgit.st of before OSWindow-Web 8
#             compiled both again, and made #webWorldDefaultAction send
#             itself), and a #webWorldDefaultAction made so is Iceberg's own
#             again after one more set up;
#   console <image>
#             the same flow on each stock image of the site (image/stock,
#             and image/stock-<id>), never set up, booted as the Console
#             boots it: st --no-source st/web-repl.st, with web-repl.st and
#             web-libgit.st in st/ next to it, as the page stages them, and
#             a line of input that evaluates git-identity.st (the prompts
#             of the REPL are taken out of what it prints); the boot must
#             have given LGitConfig its free function, so every line of
#             git-identity.txt must be a line of what it prints, as for a
#             world image, but for the two methods of the global
#             configuration, which the boot compiled in the protocols of
#             LibGit-Core that the stock ones are in ('global config
#             methods: LibGit-Core/libgit - calls LibGit-Core/accessing
#             marker true logged true frees true'); the second evaluation of
#             web-libgit.st (the set up again of git-identity.st) compiles
#             nothing, and the one after a #webWorldDefaultAction that sends
#             itself makes it Iceberg's own again;
#   notebook <image>
#             the same, booted as the notebook kernel: st --no-source
#             st/web-notebook.st, NOTEBOOK_EVENTS a file, and a request
#             that runs a cell which evaluates git-identity.st; the events
#             must be hello, start and a done of status ok, and every line
#             of git-identity.txt a line of what it prints, and the methods
#             of the global configuration as for the console;
#   prepared <image>
#             each stock image booted as the page boots one that it prepares
#             for the world (vm-worker.js: as the Console boots it, then
#             PREPARE), with web-repl.st, web-libgit.st, web-bootstrap.st and
#             the site's OSWindow-Web.st in st/: a line of the REPL files
#             web-bootstrap.st in (but does not save); the boot has put the
#             config_free: of web-libgit.st in LibGit-Core, before the
#             package was there, and the set up (#setUpLibGit) must have made
#             it an extension of OSWindow-Web, with LibGit-Core unmodified
#             and without it in its snapshot (an image whose LibGit-Core has
#             a config_free: of its own, Pharo 12, keeps it as it is); and
#             the same of config_find_global_path: and
#             globalConfigurationPath, which the boot compiled in
#             LibGit-Core on every image (Pharo 12 too: all give
#             git_config_find_global a String), and the set up again as
#             extensions of OSWindow-Web, which take them out of the
#             snapshot of LibGit-Core; and IceMergeResolutionRequest's
#             default action must be the one of web-libgit.st, which its boot
#             compiled, with Iceberg's own as #webWorldDefaultAction, which
#             does not send itself (the set up, a second evaluation of
#             web-libgit.st, compiled neither again);
#   control <image>
#             the same on each stock image of the site (image/stock, and
#             image/stock-<id>), which nothing set up: when its LGitConfig
#             frees with object_free: (Pharo 13 and later), the steps up to
#             the dialog must work and a commit must then fail (the
#             finalization may come during the first one already, at a
#             garbage collection of its own), which shows that the identity
#             check sees the bug that the set up removes; when it frees
#             with config_free: (Pharo 12), the stock image must pass as a
#             world image does, but for the global configuration; and on
#             every stock image globalConfigurationPath must answer, before
#             the dialog, a String of 256 characters with control
#             characters ('not a path: 256 characters, ...'), the bug that
#             web-libgit.st removes, from the methods of LibGit-Core, which
#             nothing compiled ('global config methods: LibGit-Core/libgit -
#             calls LibGit-Core/accessing marker false logged true frees
#             false');
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
#             with `git http-backend';
#   push <image>
#             tests/wasm/st/git-http-push.st, on the stock image as the
#             Console boots it (web-libgit.st evaluated first, which makes
#             Iceberg's provider of credentials IceNonInteractiveCredentialsProvider)
#             with the requests straight to the server, and on the world image
#             (image/web, whose provider is OSWebCredentialsProvider) through
#             the CORS proxy of git-http.mjs (PHARO_WASM_GIT_PROXY, as the user
#             would choose it), pushes to git-http.mjs --push --auth, which
#             takes a push only with HTTP Basic credentials, as github.com:
#             without credentials in IceCredentialStore the push must fail with
#             the transport's 401, which says that none was given, after one
#             request; with a wrong password stored, with its refusal and what
#             the server said ('Invalid username or token.'); with the right one
#             stored it must push (the 401 of the advertisement, then the same
#             GET and the POST of the pack with an Authorization, through the
#             proxy for the world image), and with the user name and password
#             in the URL of a second remote too (the same three requests: the
#             first without them, and no URL with them, which the proxy would
#             see); and with a provider of Iceberg's that gives a wrong password
#             then the right one, after two rounds (a 401, a 401 with the wrong
#             one, then the GET and the POST with the right one); with the same
#             git_remote of libgit2 for two pushes, the second, once nothing is
#             stored, must fail with the 401 of none given, not push with the
#             credentials of the first (the close of the subtransport at the end
#             of a push forgets them); the commits
#             must then be on the server, as its own git logs them; the
#             credentials must be in no request line nor any log, curl must
#             have read them from a file (-H @<file>), never from its command
#             line (a wrapper of curl first in PATH logs its arguments), and
#             that file must be gone from TMPDIR; and the run, traced
#             (PHARO_WASM_FFI_TRACE=1), must adapt no function that
#             adapted-66.txt does not list;
#   pull <image>
#             in the same run, once the lane committed a method on the server
#             (a handshake through files), a pull must fast-forward and load it,
#             with the working copy clean; after a local commit and another
#             commit on the server, a push must be refused (IceRemoteDesynchronized),
#             the pull must merge (a commit 'Merge ...' of 2 parents, with the
#             methods of both sides loaded) and the push of the merge must
#             land: the server's git must log the merge, of 2 parents, above
#             both commits; then, after a local commit and one of the server
#             that change the same method, the pull must fail at once with
#             IceMergeAborted (no world: the default action that web-libgit.st
#             compiles opens no dialog of the merge, which nothing would
#             answer), the head and the method left as they were, the server's
#             commit incoming.  Both are skipped as redirect is, and each needs
#             the image (no world image: push web and pull web are skipped);
#   hosts     tests/wasm/st/git-http-hosts.st on the stock image, with
#             web-libgit.st, against git-http.mjs --push --auth --mover:
#             credentials stored for localhost, the host of the mover, must
#             not reach the git server (127.0.0.1, another host): a push to a
#             remote of the mover whose advertisement it redirects there (301),
#             and one to a remote of the mover that asks for credentials (401)
#             and then redirects the request that carries them (302), straight
#             to the servers, and the latter through the proxy (which makes the
#             redirect a path of its own), must each fail with the transport's
#             'redirected the request to <git server>, ... not sent to another
#             host', with no request of the git server that had an
#             Authorization; and through a proxy of plain http:// on another
#             computer (http://0.0.0.0:<port of the proxy>/, which is no
#             loopback by its name), a push to the git server must fail with
#             the transport's refusal, after a single request, which had none;
#             and a push to a repository of the git server whose pre-receive
#             hook declines it (the server answers 200, with 'ng <ref>' in its
#             report) must fail with the transport's 'refused the push of
#             refs/heads/master: pre-receive hook declined', and what the hook
#             said, its commit not on the server.
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
# (the st files of the page FILE..., in st/ next to the image, as the page
# stages them in /pharo/st)
stage_st() {
    mkdir -p st && for f in "$@"; do cp "$SRCDIR/packaging/emscripten/st/$f" st/ || return 1; done
}
vm() {
    vm_with "$1" st --quit "$script"
}
# vm_with VM ARG...: VM on Pharo.image with the arguments ARG...
vm_with() {
    vm_bin=$1; shift
    if HOME=$(pwd)/home XDG_CONFIG_HOME=$(pwd)/home/.config \
	limit "$timeout" "$vm_bin" --headless Pharo.image --no-default-preferences "$@" 2>vm.err
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
# booted KIND: git-identity.st in an image booted as the page boots it in
# KIND, repl (the Console) or notebook (the kernel), from st/web-KIND.st
# next to the image, with st/web-libgit.st: a line of the REPL, or the
# request of a cell, evaluates it.  The prompts of the REPL are taken out of
# what it prints; the events of the kernel go to events.txt
booted() (
    stage_st web-libgit.st "web-$1.st" || exit 1
    evaluate="Smalltalk compiler evaluate: '$SRCDIR/tests/wasm/st/git-identity.st' asFileReference contents"
    case $1 in
	repl) printf '%s\n' "$evaluate" >input.txt ;;
	notebook) printf '{"op":"run","rid":1,"name":"In[1]","code":"%s"}\n' "$evaluate" >input.txt ;;
    esac
    NOTEBOOK_EVENTS=$(pwd)/events.txt
    export NOTEBOOK_EVENTS
    vm_with "$WASM_DIR/node/pharo" st --no-source "st/web-$1.st" <input.txt >booted.out
    booted_status=$?
    sed 's/^\(st> \)*//' booted.out
    exit $booted_status
)
# prepared: the image booted as the page boots one that it prepares for the
# world (vm-worker.js: as the Console boots it, then the evaluation PREPARE),
# with st/ as the page stages it, web-repl.st, web-libgit.st,
# web-bootstrap.st and the site's OSWindow-Web.st: a line of the REPL files
# web-bootstrap.st in, as PREPARE does but for its save, and the next one
# prints 'prepared: <major> <version of OSWindow-Web> <free function> <its
# package>/<its protocol> ext <an extension> modified <LibGit-Core modified
# for Monticello> snapshot <in the snapshot of LibGit-Core> marker
# <compiled by web-libgit.st>', and the next one 'prepared find: <its
# package>/<its protocol> ext <an extension> snapshot <in the snapshot of
# LibGit-Core> marker <compiled by web-libgit.st>' of
# LGitConfig>>#config_find_global_path:, then the same of
# LGitConfig>>#globalConfigurationPath, then ' modified <LibGit-Core modified
# for Monticello>', and the next one 'prepared merge: <compiled by
# web-libgit.st> <sends #webWorldDefaultAction>' of
# IceMergeResolutionRequest>>#defaultAction, then the same of
# #webWorldDefaultAction
prepared_boot() (
    stage_st web-libgit.st web-repl.st web-bootstrap.st || exit 1
    cp "$WASM_DIR/web/st/OSWindow-Web.st" st/ || exit 1
    {
	echo "CodeImporter evaluateFileNamed: (FileLocator imageDirectory / 'st' / 'web-bootstrap.st') fullName. nil"
	echo "| m lgc in | m := LGitConfig class lookupSelector: LGitConfig freeFunctionSelector. lgc := MCWorkingCopy forPackageNamed: 'LibGit-Core'." \
	    "in := lgc package snapshot definitions anySatisfy: [ :d | d isMethodDefinition and: [ d className = #LGitConfig and: [ d classIsMeta and: [ d selector = m selector ] ] ] ]." \
	    "Stdio stdout nextPutAll: 'prepared: ' , SystemVersion current major printString , ' ' , OSWebDriver packageVersion printString , ' ' ," \
	    "LGitConfig freeFunctionSelector , ' ' , m package name , '/' , m protocolName , ' ext ' , m isExtension printString ," \
	    "' modified ' , lgc modified printString , ' snapshot ' , in printString ," \
	    "' marker ' , ((m sourceCode ifNil: [ '' ]) includesSubstring: 'compiled by web-libgit.st') printString; lf; flush. nil"
	echo "| lgc | lgc := MCWorkingCopy forPackageNamed: 'LibGit-Core'. Stdio stdout nextPutAll: 'prepared find:'." \
	    "#( #config_find_global_path: #globalConfigurationPath ) do: [ :s | | m in |" \
	    "m := LGitConfig >> s. in := lgc package snapshot definitions anySatisfy: [ :d | d isMethodDefinition and: [ d className = #LGitConfig and: [ d classIsMeta not and: [ d selector = s ] ] ] ]." \
	    "Stdio stdout nextPutAll: ' ' , m package name , '/' , m protocolName , ' ext ' , m isExtension printString , ' snapshot ' , in printString ," \
	    "' marker ' , ((m sourceCode ifNil: [ '' ]) includesSubstring: 'compiled by web-libgit.st') printString ]." \
	    "Stdio stdout nextPutAll: ' modified ' , lgc modified printString; lf; flush. nil"
	echo "Stdio stdout nextPutAll: 'prepared merge:'. #( #defaultAction #webWorldDefaultAction ) do: [ :s | | m |" \
	    "m := IceMergeResolutionRequest compiledMethodAt: s ifAbsent: [ nil ]. Stdio stdout nextPutAll: ' ' , (m isNil" \
	    "ifTrue: [ 'none' ] ifFalse: [ ((m sourceCode ifNil: [ '' ]) includesSubstring: 'web-libgit.st') printString , ' ' ," \
	    "(m ast allChildren anySatisfy: [ :n | n isMessage and: [ n selector = #webWorldDefaultAction ] ]) printString ]) ]." \
	    "Stdio stdout lf; flush. nil"
    } >input.txt
    vm_with "$WASM_DIR/node/pharo" st --no-source st/web-repl.st <input.txt >booted.out
    booted_status=$?
    sed 's/^\(st> \)*//' booted.out
    exit $booted_status
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
# global_methods NAME EXPECTED: fails NAME and answers false when the line
# 'global config methods' of smoke.out (git-identity.st) is not EXPECTED
global_methods() {
    if test "x$(value 'global config methods')" != "x$2"; then
	fail "$1" "the methods of the global configuration are '$(value 'global config methods' | cut -c 1-200)', not '$2'"
	return 1
    fi
    return 0
}
# not_a_path KEY: whether the line KEY of smoke.out (git-identity.st) shows
# what a stock LibGit-Core answers, a String of 256 characters, the bytes of
# the git_buf of libgit2 among them
not_a_path() {
    case $(value "$1") in
	'not a path: 256 characters, '*) return 0 ;;
    esac
    return 1
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

# unlisted: the functions of trace.err that adapted-66.txt does not list,
# each after a blank
unlisted() {
    known=$(grep -v '^#' "$adapted_list" | awk 'NF { print $1 }')
    for name in $(adapted); do
	echo "$known" | grep -qxF "$name" || printf ' %s' "$name"
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
    stage_st web-libgit.st
    run "identity $image" identity && quiet "identity $image" && {
	if ! differ smoke.out "$identity_golden"; then
	    fail "identity $image" "differs from ${identity_golden#$SRCDIR/}: $(tr '\n' ';' <differ.txt | cut -c 1-1500)"
	elif global_methods "identity $image" 'OSWindow-Web/*OSWindow-Web OSWindow-Web/*OSWindow-Web marker true logged true frees true'; then
	    ok "identity $image" "Pharo $(value major), frees with $(value 'free calls'), $(value dialogs) dialog, commits $(value 'commit 1') $(value 'commit 2') $(value 'commit 3') by $(value gitconfig) after the finalization; global config $(value 'global config before'), then $(value 'global config')"
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
	elif ! global_methods "control $image" 'LibGit-Core/libgit - calls LibGit-Core/accessing marker false logged true frees false'; then
	    :
	elif ! not_a_path 'global config before'; then
	    fail "control $image" "globalConfigurationPath of a stock LibGit-Core answered '$(value 'global config before' | cut -c 1-200)' before the dialog, not the 256 characters of the String that libgit2 wrote its git_buf into: the identity check would not see the bug"
	elif test "x$free" = 'xobject_free:'; then
	    case $errors in
		*"'commit 1'"*|*"'signature'"*|*"'commit 2'"*|*"'commit 3'"*)
		    ok "control $image" "Pharo $(value major), LGitConfig frees with #object_free:, so $errors failed, with $(grep -c '' guard.err) failed callouts; globalConfigurationPath answers '$(value 'global config before')'" ;;
		*) fail "control $image" "LGitConfig frees with object_free:, but no commit failed (errors: '$(echo "$errors" | cut -c 1-200)'): the identity check would not see the bug" ;;
	    esac
	elif test "x$free" = 'xconfig_free:'; then
	    if test -s guard.err; then
		fail "control $image" "LGitConfig frees with config_free:, but $(grep -c '' guard.err) callouts failed: $(head -n 1 guard.err)"
	    elif ! grep -v '^global config\|^merge ' "$identity_golden" >control-golden.txt ||
		    ! differ smoke.out control-golden.txt; then
		fail "control $image" "LGitConfig frees with config_free:, but the flow differs from ${identity_golden#$SRCDIR/} (but for the global config and the merge): $(tr '\n' ';' <differ.txt | cut -c 1-1000)"
	    elif ! not_a_path 'global config'; then
		fail "control $image" "globalConfigurationPath of a stock LibGit-Core answered '$(value 'global config' | cut -c 1-200)' after the dialog, not the 256 characters of the String that libgit2 wrote its git_buf into"
	    else
		ok "control $image" "Pharo $(value major), LGitConfig frees with #config_free: already, and the flow passes; globalConfigurationPath answers '$(value 'global config before')', then '$(value 'global config')'"
	    fi
	else
	    fail "control $image" "LGitConfig frees with '$(echo "$free" | cut -c 1-200)'"
	fi
    }
done

# The console and notebook checks, on the stock images, which only the boot
# sets up
for dir in "$WASM_DIR/image/stock" "$WASM_DIR"/image/stock-*; do
    ls "$dir"/*.image >/dev/null 2>&1 || continue
    image=${dir##*/}
    for kind in console notebook; do
	fresh "$kind-$image" "$dir"
	empty_home
	case $kind in console) mode=repl ;; *) mode=notebook ;; esac
	run "$kind $image" booted $mode && quiet "$kind $image" && {
	    events=
	    if test $kind = notebook; then
		events=$(sed -n 's/^{"ev":"\([a-z]*\)".*/\1/p' events.txt 2>/dev/null | tr '\n' ' ')
		grep -q '^{"ev":"done","rid":1,.*"status":"ok"' events.txt 2>/dev/null || events="$events(no done of status ok)"
	    fi
	    if test $kind = notebook && test "x$events" != 'xhello start done '; then
		fail "$kind $image" "the events of the kernel are '$events', not hello, start and a done of status ok"
	    elif ! differ smoke.out "$identity_golden"; then
		fail "$kind $image" "differs from ${identity_golden#$SRCDIR/}: $(tr '\n' ';' <differ.txt | cut -c 1-1500)"
	    elif global_methods "$kind $image" 'LibGit-Core/libgit - calls LibGit-Core/accessing marker true logged true frees true'; then
		ok "$kind $image" "Pharo $(value major), booted as the $kind boots it: frees with $(value 'free calls'), $(value dialogs) dialog, commits $(value 'commit 1') $(value 'commit 2') $(value 'commit 3') by $(value gitconfig) after the finalization; global config $(value 'global config before'), then $(value 'global config')"
	    fi
	}
    done
done

# The prepared checks, on the stock images: booted as the page boots one
# that it prepares for the world
for dir in "$WASM_DIR/image/stock" "$WASM_DIR"/image/stock-*; do
    ls "$dir"/*.image >/dev/null 2>&1 || continue
    image=${dir##*/}
    fresh "prepared-$image" "$dir"
    empty_home
    run "prepared $image" prepared_boot && quiet "prepared $image" && {
	got=$(sed -n 's/^prepared: //p' smoke.out | head -n 1)
	find=$(sed -n 's/^prepared find: //p' smoke.out | head -n 1)
	merge=$(sed -n 's/^prepared merge: //p' smoke.out | head -n 1)
	extension='OSWindow-Web/*OSWindow-Web ext true snapshot false marker true'
	if test "x$find" != "x$extension $extension modified false"; then
	    fail "prepared $image" "config_find_global_path: and globalConfigurationPath are '$(echo "$find" | cut -c 1-300)', not extensions of OSWindow-Web compiled by web-libgit.st, out of the snapshot of an unmodified LibGit-Core"
	elif test "x$merge" != 'xtrue true false false'; then
	    fail "prepared $image" "the default action of IceMergeResolutionRequest and #webWorldDefaultAction are '$(echo "$merge" | cut -c 1-200)' (compiled by web-libgit.st, sends #webWorldDefaultAction), not 'true true false false': the one of web-libgit.st, which sends #webWorldDefaultAction in a world, and Iceberg's own there, which does not send itself"
	else case $got in
	    *' config_free: OSWindow-Web/*OSWindow-Web ext true modified false snapshot false marker true')
		ok "prepared $image" "$got: web-libgit.st compiled it at the boot, in LibGit-Core, and again as an extension once OSWindow-Web was in; config_find_global_path: and globalConfigurationPath too" ;;
	    *' config_free: LibGit-Core/'*' ext false modified false snapshot true marker false')
		ok "prepared $image" "$got: the image's own free function, left as it is; config_find_global_path: and globalConfigurationPath compiled at the boot, in LibGit-Core, and again as extensions once OSWindow-Web was in" ;;
	    *) fail "prepared $image" "LGitConfig frees with '$(echo "$got" | cut -c 1-300)', not the config_free: of web-libgit.st as an extension of OSWindow-Web (nor one of LibGit-Core itself)" ;;
	esac; fi
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
# start_server [ARG...]: starts it in the current directory, by default with
# the repository r in the redirect mode (else with the options ARG...), and
# sets git_url and proxy_url; false when it did not start (see server.err)
start_server() {
    test $# -gt 0 || set -- --redirect --seed r
    "$NODE" "$SRCDIR/tests/wasm/lib/git-http.mjs" repos "$@" --log server.log \
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
    proxy_url=$(sed -n 's/^CORS proxy \([^ ]*\).*/\1/p' server.out)
    test -n "$git_url" && test -n "$proxy_url"
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

# The push and pull checks: git-http.mjs with a push that asks for the
# credentials push_user:push_password, its repository p, and the run of
# git-http-push.st, on the stock image as the Console boots it (straight to
# the server) and on the world image (through the proxy)
push_user=wasm-pusher
push_password=s3cret-wasm-token
# (the host's git, without the configuration of the user who runs the lane)
host_git() {
    GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null GIT_TERMINAL_PROMPT=0 \
	git -c user.name='Lane 66' -c user.email=lane66@example.org "$@"
}
# server_commit METHOD VALUE MESSAGE: a commit on the server's master that
# adds the method METHOD (answering VALUE) to WasmPushDemo, which the VM
# pushed, as Tonel has it; with the METHOD one, makes #one answer VALUE
# instead
server_commit() {
    rm -rf work && host_git clone -q -b master repos/p.git work && {
	if test "$1" = one; then
	    tab=$(printf '\t')
	    # (Tonel writes the method on a line of its own, or on the line of
	    # its selector, as the Pharo goes)
	    sed -e "s/^$tab\\^ 1\$/$tab^ $2/" -e "s/^\\(WasmPushDemo >> one \\[ \\)\\^ 1\$/\\1^ $2/" \
		work/src/WasmPushDemo/WasmPushDemo.class.st >work/one.st &&
		mv work/one.st work/src/WasmPushDemo/WasmPushDemo.class.st
	else
	    printf "\n{ #category : 'accessing' }\nWasmPushDemo >> %s [\n\n\t^ %s\n]\n" "$1" "$2" \
		>>work/src/WasmPushDemo/WasmPushDemo.class.st
	fi
    } && host_git -C work commit -q -am "$3" && host_git -C work push -q origin master
}
# answer: the lane's side of the handshake, in the background while the VM
# runs: each sync/<name>.wait gets its commit on the server, then
# sync/<name>.done (a minute at most for each)
answer() (
    for step in remote-ff:two:2 remote-div:four:4 remote-conflict:one:111; do
	name=${step%%:*} rest=${step#*:}
	waited=0
	while ! test -f "sync/$name.wait"; do
	    test "$waited" -ge 600 && exit 1
	    waited=$((waited + 1))
	    sleep 0.1
	done
	server_commit "${rest%%:*}" "${rest#*:}" "$name" >"sync/$name.log" 2>&1
	touch "sync/$name.done"
    done
)
# wrap_curl: bin/curl, a wrapper of curl that logs the arguments of each run
# to curl.args, one a line, then a line '--end--' (never the file of the
# header, which -H @<file> names), first in PATH
wrap_curl() {
    real_curl=$(command -v curl) && mkdir -p bin && {
	printf '#!/bin/sh\nfor a in "$@"; do printf "%%s\\n" "$a"; done >>"%s"\necho --end-- >>"%s"\nexec "%s" "$@"\n' \
	    "$(pwd)/curl.args" "$(pwd)/curl.args" "$real_curl" >bin/curl
    } && chmod +x bin/curl
}
# curl_read_file: whether curl.args has a '-H @<file>', and no argument with
# Authorization, the user or the password
curl_read_file() {
    test -s curl.args &&
	awk 'prev == "-H" && /^@/ { found = 1 } { prev = $0 } END { exit !found }' curl.args &&
	! grep -qi -e authorization -e "$push_password" -e "$push_user" curl.args
}
# http_push VIA: the run of git-http-push.st, VIA direct (and web-libgit.st
# evaluated first) or proxy
http_push() (
    unset PHARO_WASM_GIT_PROXY WASM_WEB_LIBGIT
    PHARO_WASM_FFI_TRACE=1
    export PHARO_WASM_FFI_TRACE
    case $1 in
	proxy) PHARO_WASM_GIT_PROXY=$proxy_url; export PHARO_WASM_GIT_PROXY ;;
	*) WASM_WEB_LIBGIT=$SRCDIR/packaging/emscripten/st/web-libgit.st; export WASM_WEB_LIBGIT ;;
    esac
    script=$SRCDIR/tests/wasm/st/git-http-push.st
    WASM_GIT_PUSH_REMOTE=${git_url}p.git WASM_GIT_PUSH_USER=$push_user WASM_GIT_PUSH_PASSWORD=$push_password
    WASM_GIT_SYNC=$(pwd)/sync TMPDIR=$(pwd)/tmp PATH=$(pwd)/bin:$PATH
    export WASM_GIT_PUSH_REMOTE WASM_GIT_PUSH_USER WASM_GIT_PUSH_PASSWORD WASM_GIT_SYNC TMPDIR PATH
    answer &
    answer_pid=$!
    vm "$WASM_DIR/node/pharo"
    vm_status=$?
    kill "$answer_pid" 2>/dev/null
    wait "$answer_pid" 2>/dev/null
    exit $vm_status
)
# push_checks IMAGE VIA: the push and the pull checks of IMAGE (a directory of
# $WASM_DIR/image)
push_checks() {
    image=$1 via=$2
    fresh "push-$image" "$WASM_DIR/image/$image"
    home
    mkdir -p repos sync tmp
    if ! wrap_curl; then
	checks=$((checks + 2))
	fail "push $image" "could not make the wrapper of curl"
	fail "pull $image" "could not make the wrapper of curl"
	return
    fi
    if ! start_server --push --auth "$push_user:$push_password" --seed p; then
	checks=$((checks + 2))
	fail "push $image" "the git server did not start: $(head -n 1 server.err | cut -c 1-200)"
	fail "pull $image" "the git server did not start"
	return
    fi
    if ! { run "push $image" http_push "$via" && quiet "push $image"; }; then
	stop_server
	checks=$((checks + 1))
	fail "pull $image" "the run failed (see push $image)"
	return
    fi
    stop_server
    host=${git_url#http://}
    host=${host%/}
    receive="GET /p.git/info/refs?service=git-receive-pack"
    # (the requests of the git server, and of the proxy, for the push)
    grep '^git ' server.log >git.log
    grep '^proxy ' server.log >proxy.log
    host_git -C repos/p.git log --format='%s (%p)' master >remote.log 2>&1
    none=$(value 'push none') wrong=$(value 'push wrong')
    if test "x$(value provider)" != "x$(test "$via" = proxy && echo OSWebCredentialsProvider || echo IceNonInteractiveCredentialsProvider)"; then
	fail "push $image" "the provider of credentials is '$(value provider | cut -c 1-100)'"
    elif test "x$(value clone)" != xmaster || test "x$(value 'commit 1')" != 'xpushed from wasm'; then
	fail "push $image" "the clone or the commit: '$(value clone | cut -c 1-200)', '$(value 'commit 1' | cut -c 1-200)'"
    elif case $none in "error "*"git http: $host answered 401: "*"none was given"*"$host says: No anonymous write access.") false ;; *) true ;; esac; then
	fail "push $image" "without credentials, not the transport's 401 of none given: '$(echo "$none" | cut -c 1-300)'"
    elif case $wrong in "error "*"git http: $host refused the user name and password or token (401); $host says: Invalid username or token.") false ;; *) true ;; esac; then
	fail "push $image" "with a wrong password, not the transport's refusal: '$(echo "$wrong" | cut -c 1-300)'"
    elif test "x$(value 'push stored')" != xok || test "x$(value 'push url')" != xok ||
	test "x$(value 'push rounds')" != 'xok, answers left 0'; then
	fail "push $image" "with the stored credentials: '$(value 'push stored' | cut -c 1-200)', with those of the URL: '$(value 'push url' | cut -c 1-200)', with a second round: '$(value 'push rounds' | cut -c 1-200)'"
    elif test "x$(value 'push reuse 1')$(value 'push reuse 3')" != xokok ||
	case $(value 'push reuse 2') in "error "*"git http: $host answered 401: "*"none was given"*) false ;; *) true ;; esac; then
	fail "push $image" "two pushes through the same git_remote: '$(value 'push reuse 1' | cut -c 1-100)', then with nothing stored '$(value 'push reuse 2' | cut -c 1-200)' (the 401 of none given, not the credentials of the first), then '$(value 'push reuse 3' | cut -c 1-100)'"
    elif test "x$(sed -n 3p git.log)" != "xgit $receive 401"; then
	fail "push $image" "the push without credentials did not ask once, without them: $(sed -n 3,4p git.log | tr '\n' ';')"
    elif test "x$(sed -n 4,8p git.log | tr '\n' ';')" != "xgit $receive 401;git $receive 401 authorization;git $receive 401;git $receive 200 authorization;git POST /p.git/git-receive-pack 200 authorization;"; then
	fail "push $image" "the wrong password, then the right one: not a 401, the same GET with an Authorization, then the POST with it: $(sed -n 4,8p git.log | tr '\n' ';')"
    elif test "x$(sed -n 9,11p git.log | tr '\n' ';')" != "xgit $receive 401;git $receive 200 authorization;git POST /p.git/git-receive-pack 200 authorization;"; then
	fail "push $image" "the credentials of the URL: not a request without them first, then the same GET and the POST with an Authorization: $(sed -n 9,11p git.log | tr '\n' ';')"
    elif test "x$(sed -n 12,15p git.log | tr '\n' ';')" != "xgit $receive 401;git $receive 401 authorization;git $receive 200 authorization;git POST /p.git/git-receive-pack 200 authorization;"; then
	fail "push $image" "two rounds of credentials: not a 401, a 401 with the wrong ones, then the GET and the POST with the right ones: $(sed -n 12,15p git.log | tr '\n' ';')"
    elif test "$via" = proxy && ! grep -qx "proxy POST /$host/p.git/git-receive-pack 200 authorization" proxy.log; then
	fail "push $image" "the proxy did not forward the POST of the pack with its Authorization: $(tr '\n' ';' <proxy.log | cut -c 1-400)"
    elif test "$via" != proxy && test -s proxy.log; then
	fail "push $image" "requests went to the proxy: $(head -n 2 proxy.log | tr '\n' ';')"
    elif ! grep -q '^second push (' remote.log || ! grep -q '^pushed from wasm (' remote.log; then
	fail "push $image" "the server's git does not log the pushed commits: $(tr '\n' ';' <remote.log | cut -c 1-300)"
    elif grep -q "$push_password\|$push_user" server.log smoke.out smoke.err; then
	fail "push $image" "the credentials are in a log: $(grep -h "$push_password\|$push_user" server.log smoke.out smoke.err | head -n 1 | cut -c 1-200)"
    elif test -n "$(ls tmp)"; then
	fail "push $image" "curl's header is left in TMPDIR: $(ls tmp | tr '\n' ' ')"
    elif ! curl_read_file; then
	fail "push $image" "curl did not read the credentials from a file (-H @<file>), or got them on its command line: $(grep -i -m 1 -e authorization -e '^@' curl.args | cut -c 1-100)"
    elif test -n "$(unlisted)"; then
	fail "push $image" "adapted functions that ${adapted_list#$SRCDIR/} does not list:$(unlisted)"
    else
	ok "push $image" "$(test "$via" = proxy && echo 'through the proxy' || echo 'straight to the server'), with $(value provider): a 401 of none given, a refusal of the wrong password, then pushed with the stored credentials, with those of the URL, and after two rounds, the same git_remote asked again, $(grep -c ' authorization$' git.log) requests with credentials, curl's from a file"
    fi
    checks=$((checks + 1))
    if test "x$(value 'pull ff')" != xok || test "x$(value 'ff head')" != 'xremote-ff, parents 1' ||
	test "x$(value 'ff selectors')" != 'x#(#one #reuse #rounds #two #zero)' || test "x$(value 'ff modified')" != xfalse; then
	fail "pull $image" "the fast-forward: pull '$(value 'pull ff' | cut -c 1-200)', head '$(value 'ff head')', selectors '$(value 'ff selectors')', modified '$(value 'ff modified')'"
    elif case $(value 'push behind') in 'error IceRemoteDesynchronized'*) false ;; *) true ;; esac; then
	fail "pull $image" "a push behind the server was not refused: '$(value 'push behind' | cut -c 1-200)'"
    elif test "x$(value 'pull merge')" != xok || test "x$(value 'merge head')" != 'xMerge, parents 2' ||
	test "x$(value 'merge selectors')" != 'x#(#four #one #reuse #rounds #three #two #zero)' || test "x$(value 'merge modified')" != xfalse; then
	fail "pull $image" "the merge: pull '$(value 'pull merge' | cut -c 1-200)', head '$(value 'merge head')', selectors '$(value 'merge selectors')', modified '$(value 'merge modified')'"
    elif test "x$(value 'push merge')" != xok || test "x$(value after)" != x7; then
	fail "pull $image" "the push of the merge: '$(value 'push merge' | cut -c 1-200)', after '$(value after)'"
    elif ! sed -n 2p remote.log | grep -q '^Merge [0-9a-f]* ([0-9a-f]* [0-9a-f]*)$' || ! head -n 1 remote.log | grep -q '^remote-conflict (' ||
	! sed -n 3,4p remote.log | grep -q '^local change (' || ! sed -n 3,4p remote.log | grep -q '^remote-div ('; then
	fail "pull $image" "the server's git does not log the merge of 2 parents above both commits (under remote-conflict): $(tr '\n' ';' <remote.log | cut -c 1-300)"
    elif case $(value 'pull conflict') in 'error IceMergeAborted: '*'the merge has conflicts, which Iceberg resolves in a dialog that only the world shows'*) false ;; *) true ;; esac; then
	fail "pull $image" "a pull whose merge conflicts did not fail with the IceMergeAborted of web-libgit.st: '$(value 'pull conflict' | cut -c 1-300)'"
    elif test "x$(value 'conflict head')" != 'xlocal one, parents 1, one 11' || test "x$(value 'conflict incoming')" != x1; then
	fail "pull $image" "after the conflict, not as it was: head '$(value 'conflict head' | cut -c 1-200)', incoming '$(value 'conflict incoming' | cut -c 1-100)'"
    else
	ok "pull $image" "fast-forward, then a merge of 2 parents pushed: $(sed -n 2,4p remote.log | sed 's/ ([^)]*)//' | tr '\n' ';' | cut -c 1-120); a conflict aborted the next pull at once"
    fi
}
if ! grep -Eiq '^WASM_LIBGIT2_HTTP:BOOL=(ON|TRUE|YES|Y|1)$' "$WASM_DIR/cmake/CMakeCache.txt" 2>/dev/null; then
    push_skip="no smart-HTTP transport (WASM_LIBGIT2_HTTP=OFF)"
elif ! command -v curl >/dev/null 2>&1; then
    push_skip="no curl, which the node VM makes its HTTP requests with"
elif ! has_git; then
    push_skip="no git, or no git http-backend, to serve the repositories"
else
    push_skip=
fi
for pushed in stock:direct web:proxy; do
    image=${pushed%%:*}
    if test -n "$push_skip"; then
	skip_check "push $image" "$push_skip"
	skip_check "pull $image" "$push_skip"
    elif ! ls "$WASM_DIR/image/$image"/*.image >/dev/null 2>&1; then
	skip_check "push $image" "no image in $WASM_DIR/image/$image (WASM_WORLD=OFF, or no WASM_HOST_PHARO)"
	skip_check "pull $image" "no image in $WASM_DIR/image/$image"
    else
	push_checks "$image" "${pushed#*:}"
	stop_server
    fi
done

# The hosts check: git-http-hosts.st on the stock image, with web-libgit.st,
# against git-http.mjs --push --auth --mover (mover_url, the mover, named
# localhost); hosts_run NAME PROXY REMOTE...: a run of it, its output in
# NAME.out, with PHARO_WASM_GIT_PROXY=PROXY unless PROXY is empty
hosts_run() (
    name=$1 via=$2
    shift 2
    unset PHARO_WASM_GIT_PROXY
    test -n "$via" && { PHARO_WASM_GIT_PROXY=$via; export PHARO_WASM_GIT_PROXY; }
    script=$SRCDIR/tests/wasm/st/git-http-hosts.st
    WASM_WEB_LIBGIT=$SRCDIR/packaging/emscripten/st/web-libgit.st
    WASM_GIT_HOSTS_REMOTES=$* WASM_GIT_HOSTS_USER=$push_user WASM_GIT_HOSTS_PASSWORD=$push_password TMPDIR=$(pwd)/tmp
    export WASM_WEB_LIBGIT WASM_GIT_HOSTS_REMOTES WASM_GIT_HOSTS_USER WASM_GIT_HOSTS_PASSWORD TMPDIR
    vm "$WASM_DIR/node/pharo" >"$name.out"
)
# hosts_value NAME KEY: the value of the line 'KEY: ...' of NAME.out
hosts_value() {
    sed -n "s/^$2: //p" "$1.out" | head -n 1
}
if test -n "$push_skip"; then
    skip_check hosts "$push_skip"
elif ! ls "$stock"/*.image >/dev/null 2>&1; then
    skip_check hosts "no image in $stock"
else
    fresh hosts "$stock"
    home
    mkdir -p repos tmp
    checks=$((checks + 1))
    if ! start_server --push --auth "$push_user:$push_password" --mover --seed p --seed declined; then
	fail hosts "the git server did not start: $(head -n 1 server.err | cut -c 1-200)"
    else
	printf '#!/bin/sh\necho "protected branch: pushes to master are declined" >&2\nexit 1\n' >repos/declined.git/hooks/pre-receive &&
	    chmod +x repos/declined.git/hooks/pre-receive
	mover_url=$(sed -n 's/^mover \([^ ]*\).*/\1/p' server.out)
	mover=${mover_url#http://}
	mover=${mover%/}
	host=${git_url#http://}
	host=${host%/}
	proxy_port=${proxy_url##*:}
	proxy_port=${proxy_port%/}
	redirected="git http: $mover redirected the request to $host, which asks for a user name and a password or token (401): the credentials of $mover are not sent to another host"
	plain="git http: $host asks for a user name and a password or token (401), which this VM does not send through the proxy http://0.0.0.0:$proxy_port over plain http://"
	# (the runs one after the other, on the same copy of the image)
	direct_ok=0 proxied_ok=0 plain_ok=0
	: >guards.err
	hosts_run direct '' "${mover_url}adv/p.git" "${mover_url}auth/p.git" "${git_url}declined.git" 2>direct.err && direct_ok=1
	cat guard.err >>guards.err
	lines_direct=$(wc -l <server.log)
	hosts_run proxied "$proxy_url" "${mover_url}auth/p.git" 2>proxied.err && proxied_ok=1
	cat guard.err >>guards.err
	lines_proxied=$(wc -l <server.log)
	hosts_run plain "http://0.0.0.0:$proxy_port/" "${git_url}p.git" 2>plain.err && plain_ok=1
	cat guard.err >>guards.err
	stop_server
	sed -n "$((lines_proxied + 1)),\$p" server.log | grep '^git ' | grep 'git-receive-pack' >plain.log
	# (vm() leaves on stderr all but the lines of the guard and the trace)
	if test "$direct_ok$proxied_ok$plain_ok" != 111 || test -s direct.err || test -s proxied.err || test -s plain.err ||
	    test -s guards.err || test -s PharoDebug.log; then
	    fail hosts "a run failed (direct $direct_ok, proxied $proxied_ok, plain $plain_ok), or wrote on stderr, or a callout failed, or it wrote PharoDebug.log: $(cat direct.err proxied.err plain.err guards.err PharoDebug.log 2>/dev/null | head -n 1 | cut -c 1-200)"
	elif test "x$(hosts_value direct 'clone 1')$(hosts_value direct 'clone 2')$(hosts_value direct 'clone 3')$(hosts_value proxied 'clone 1')$(hosts_value plain 'clone 1')" != xmastermastermastermastermaster; then
	    fail hosts "the clones: $(grep -h '^clone' direct.out proxied.out plain.out | tr '\n' ';' | cut -c 1-400)"
	elif case $(hosts_value direct 'push 1') in "error "*"$redirected"*) false ;; *) true ;; esac; then
	    fail hosts "a push whose advertisement $mover redirects to $host, which asks for credentials: not the refusal of the redirect: '$(hosts_value direct 'push 1' | cut -c 1-300)'"
	elif case $(hosts_value direct 'push 2') in "error "*"$redirected"*) false ;; *) true ;; esac; then
	    fail hosts "a push whose request with credentials $mover redirects to $host: not the refusal of the redirect: '$(hosts_value direct 'push 2' | cut -c 1-300)'"
	elif case $(hosts_value proxied 'push 1') in "error "*"$redirected"*) false ;; *) true ;; esac; then
	    fail hosts "the same through the proxy: not the refusal of the redirect: '$(hosts_value proxied 'push 1' | cut -c 1-300)'"
	elif grep '^git ' server.log | grep -v ' /declined\.git/' | grep -q ' authorization$'; then
	    fail hosts "the git server got the credentials stored for $mover: $(grep '^git .* authorization$' server.log | grep -v ' /declined\.git/' | head -n 2 | tr '\n' ';')"
	elif test "$(sed -n "1,${lines_direct}p" server.log | grep -c '^mover GET /auth/p.git/info/refs?service=git-receive-pack 302 authorization$')" -ne 1 ||
	    test "$(sed -n "$((lines_direct + 1)),${lines_proxied}p" server.log | grep -c '^mover GET /auth/p.git/info/refs?service=git-receive-pack 302 authorization$')" -ne 1; then
	    fail hosts "$mover did not get the request with its credentials once in each run, which it redirects: $(grep '^mover' server.log | tr '\n' ';' | cut -c 1-400)"
	elif ! sed -n "$((lines_direct + 1)),${lines_proxied}p" server.log | grep -q "^proxy GET /$host/p.git/info/refs?service=git-receive-pack 401$"; then
	    fail hosts "through the proxy, the redirect did not go to $host through it, without credentials: $(sed -n "$((lines_direct + 1)),${lines_proxied}p" server.log | grep '^proxy' | tr '\n' ';' | cut -c 1-400)"
	elif case $(hosts_value plain 'push 1') in "error "*"$plain"*) false ;; *) true ;; esac; then
	    fail hosts "through a proxy of plain http:// on another computer: not the refusal of the transport: '$(hosts_value plain 'push 1' | cut -c 1-300)'"
	elif test "x$(tr '\n' ';' <plain.log)" != "xgit GET /p.git/info/refs?service=git-receive-pack 401;"; then
	    fail hosts "through a proxy of plain http://, not a single request of the push, without credentials: $(tr '\n' ';' <plain.log | cut -c 1-300)"
	elif case $(hosts_value direct 'push 3') in "error "*"git http: $host refused the push of refs/heads/master: pre-receive hook declined; $host says: protected branch: pushes to master are declined") false ;; *) true ;; esac; then
	    fail hosts "a push that the hook of the server declines: not the transport's refusal of the ref: '$(hosts_value direct 'push 3' | cut -c 1-300)'"
	elif ! grep -qx "git POST /declined.git/git-receive-pack 200 authorization" server.log ||
	    test "x$(host_git -C repos/declined.git log --format=%s master 2>&1 | tr '\n' ';')" != 'xfirst;'; then
	    fail hosts "the declined push: not a POST answered 200, or its commit is on the server: $(host_git -C repos/declined.git log --format=%s master 2>&1 | tr '\n' ';' | cut -c 1-200)"
	elif test "x$(hosts_value plain after)" != x7 || test -n "$(ls tmp)"; then
	    fail hosts "the VM did not go on ('$(hosts_value plain after)'), or curl's header is left in TMPDIR: $(ls tmp | tr '\n' ' ')"
	else
	    ok hosts "the credentials of $mover never reached $host (a redirected advertisement, a redirected request with them, straight and through the proxy), nor went through a proxy of plain http://; a push that the server's hook declined failed"
	fi
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
