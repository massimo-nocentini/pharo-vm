#!/bin/sh
# 67-sdl2 - the world of the images through OSSDL2Driver and SDL2, in node
#
# build-wasm/node/pharo on fresh copies of the stock image runs, with SDL's
# dummy video driver (SDL_VIDEODRIVER=dummy) and --interactive,
# tests/wasm/st/sdl-session.st: without the display of a page, the image
# opens its world through its own OSWindow-SDL2 bindings, on the library
# SDL2 of the registry (found as the placeholder node/libSDL2-2.0.so.0),
# as it does in the page sdl.html, where SDL's Emscripten video driver
# draws it instead.
#
#   session   every step of the session must say ok: the world's driver is
#             OSSDL2Driver, on SDL 2.32.10; a Playground and a Browser open;
#             the clipboard (SDL's own: SDL2 has no clipboard of the system
#             on Emscripten) gives back what it was given; the cursors, the
#             title, a resize to 1000@700 (the World and its Form follow),
#             fullscreen and back, text input on and off, the scale of the
#             screen; the Form of the world has at least 8 colours in a grid
#             of 40 by 40 of its pixels; and the events of a user, which the
#             session queues with SDL_PushEvent as SDL's Emscripten driver
#             queues those of the page: a right click on the desktop opens
#             the world menu, a click into the Playground and '3 + 4' typed
#             there (SDL_TEXTINPUT) give it that text, Ctrl+A and Ctrl+P
#             print 7;
#   web       the same session on the prepared image of the world
#             (image/web), which sdl.html boots by default: without the
#             display of the page, its OSWebDriver is not suitable, and it
#             opens its world through OSSDL2Driver too;
#   surface   SurfacePlugin is one of the builtin modules of the VM: a build
#             with SDL2 has it built in (cmake/plugins.cmake), as
#             OSWindow-SDL2 and Athens register their surfaces with it;
#   hidden    the negative control of the registry: with SDL2 hidden from
#             the VM (PHARO_WASM_FFI_HIDE=SDL2), the first callout of SDL2
#             (SDL2 version, without the world) must fail with a
#             SymbolNotFoundError for its function (SDL_Init on Pharo 12,
#             SDL_GetVersion on Pharo 15), without a crash or a failed
#             callout, and the VM must go on.  (A world would not open: the
#             stock image picks OSSDL2Driver, since the file
#             libSDL2-2.0.so.0 is there, and its start then fails to
#             initialize SDL2, before any script runs.)
#
# Every run must exit with 0, print no line of the guard of the callouts
# ('FFI callout failed, its declaration does not match ...' or 'FFI callout
# trapped in ...'), nothing else on stderr, and write no PharoDebug.log.
# The time of the session is printed with its check.
#
# Skipped when the build has no SDL2 (WASM_SDL2=OFF, the default, or
# WASM_FFI=OFF): its node VM's registry (cmake/wasm/ffi/ffiRegistry-node.c)
# has no library SDL2.
#
# Environment (from make wasm-check): NODE, WASM_DIR, SRCDIR, TEST_DIR, and
# WASM_CHECK_TIMEOUT (see lib/common.sh).

SRCDIR=$(cd "${SRCDIR:-$(dirname "$0")/../../..}" && pwd)
TEST_DIR=${TEST_DIR:+$TEST_DIR/67-sdl2}
. "$SRCDIR/tests/wasm/lib/common.sh"
lane=sdl2

if ! test -x "$WASM_DIR/node/pharo"; then
    echo "FAIL 67-sdl2: no $WASM_DIR/node/pharo (make wasm)"
    exit 1
fi
if ! grep -Eiq '^FEATURE_FFI:BOOL=(ON|TRUE|YES|Y|1)$' "$WASM_DIR/cmake/CMakeCache.txt" 2>/dev/null; then
    skip "no FFI: the build has none (WASM_FFI=OFF), so no SDL2"
fi
registry=$WASM_DIR/cmake/wasm/ffi/ffiRegistry-node.c
if ! test -f "$registry"; then
    echo "FAIL 67-sdl2: no $registry, the registry of the node VM (make wasm)"
    exit 1
fi
if ! grep -q '^extern const PharoFFILibrary pharoFFILibrary_SDL2;$' "$registry"; then
    skip "no SDL2: the registry of the node VM has no library SDL2 (WASM_SDL2=OFF)"
fi
echo "67-sdl2: $WASM_DIR/node/pharo"

script=$SRCDIR/tests/wasm/st/sdl-session.st

# The lines of the guard of the callouts, and what the runtime of a debug
# build says of itself, as in lane 64: vm() keeps the guard's in guard.err,
# out of the stderr the checks see (the message of what threw may name an
# error of the JavaScript runtime, see engine_errors in lib/common.sh), and
# leaves the runtime's out; the rest of stderr goes on.
guard='^FFI callout (failed, its declaration does not match|trapped in) '
runtime='^(Heap resize call from [0-9]+ to [0-9]+ took .* Success: true|program exited \(with status: [0-9]+\), but keepRuntimeAlive\(\) is set)'
vm() {
    if limit "$timeout" "$WASM_DIR/node/pharo" --headless Pharo.image --no-default-preferences "$@" 2>vm.err
    then vm_status=0; else vm_status=$?; fi
    grep -E "$guard" vm.err >guard.err
    grep -Ev "$guard|$runtime" vm.err >&2
    return $vm_status
}
# (the wall time of the session, in ms, in session.ms)
now() { "$NODE" -p 'Date.now()'; }
session() (
    SDL_VIDEODRIVER=dummy; export SDL_VIDEODRIVER
    start=$(now)
    vm --interactive st "$script"
    status=$?
    echo $(($(now) - start)) >session.ms
    exit $status
)
# (the version of SDL, or the error that asking for it raised)
version="[ | v | v := SDL2 version. v major printString , '.' , v minor printString , '.' , v patch printString ]
    on: Error do: [ :e | e class name , ': ' , e messageText asString ]"
hidden() ( PHARO_WASM_FFI_HIDE=SDL2; export PHARO_WASM_FFI_HIDE; vm eval "$version" )

# value KEY: the value of the line 'KEY: ...' of smoke.out
value() {
    sed -n "s/^$1: //p" smoke.out | head -n 1
}

# quiet NAME: fails NAME and answers false after a run that did not exit
# with 0, printed a line of the guard or anything else on stderr, or wrote
# PharoDebug.log
quiet() {
    if test "$status" -ne 0; then
	fail "$1" "exit status $status"
    elif test -s guard.err; then
	fail "$1" "$(grep -c '' guard.err) failed callouts: $(head -n 1 guard.err)"
    elif test -s smoke.err; then
	fail "$1" "stderr is not empty: $(head -n 1 smoke.err | cut -c 1-200)"
    elif test -s PharoDebug.log; then
	fail "$1" "it wrote PharoDebug.log: $(grep -m 1 . PharoDebug.log | cut -c 1-200)"
    else
	return 0
    fi
    return 1
}

# The steps of the session, and the lines that it must print
steps='playground browser clipboard cursor title resize fullscreen textinput dpi drawn menu type print'
# wrong: what of smoke.out is not what the session must print, in wrong.txt;
# answers false when it wrote anything
wrong() {
    : >wrong.txt
    for line in 'driver: OSSDL2Driver' 'sdl: 2.32.10' 'clipboard: hello from pharo' \
	'extent: (1000@700) display (1000@700)' 'typed: 3 + 4' 'session: done'; do
	grep -qxF "$line" smoke.out || echo "expected '$line', got '$(value "${line%%: *}" | cut -c 1-120)'" >>wrong.txt
    done
    for step in $steps; do
	if ! grep -qxF "ok $step" smoke.out; then
	    said=$(grep -m 1 "^error $step: " smoke.out | cut -c 1-200)
	    echo "${said:-step $step: no 'ok $step'}" >>wrong.txt
	fi
    done
    case $(value printed) in
	*"'7'"*) ;;
	*) echo "Ctrl+P printed '$(value printed | cut -c 1-120)', not 7" >>wrong.txt ;;
    esac
    ! test -s wrong.txt
}

# checked NAME: the session that ran for check NAME
checked() {
    quiet "$1" || return
    if wrong; then
	ok "$1" "Pharo $(value major), SDL $(value sdl), $(echo $steps | wc -w) steps, $(value colours) colours in the grid, the menu of $(value menu), $(cat session.ms) ms"
    else
	fail "$1" "$(sort -u wrong.txt | tr '\n' ';' | cut -c 1-1500)"
    fi
}

fresh session
run session session && checked session

web=$WASM_DIR/image/web
if ls "$web"/*.image >/dev/null 2>&1; then
    fresh web "$web"
    run web session && checked web
else
    skip_check web "no image in $web (WASM_WORLD=OFF, or no WASM_HOST_PHARO)"
fi

fresh surface
expect surface true "$WASM_DIR/node/pharo" --headless Pharo.image --no-default-preferences eval \
    "Smalltalk vm listBuiltinModules anySatisfy: [ :module | module beginsWith: 'SurfacePlugin' ]"

fresh hidden
run hidden hidden && quiet hidden && {
    # (Pharo 12 fails at SDL_Init, which it calls first, Pharo 15 at
    # SDL_GetVersion, which it names with a Symbol)
    if ! grep -Eq "^#'SymbolNotFoundError: Could not find symbol named: (''|#)SDL_[A-Za-z]+" smoke.out; then
	fail hidden "with SDL2 hidden, SDL2 version did not fail with a SymbolNotFoundError for a function of SDL: '$(head -n 1 smoke.out | cut -c 1-200)'"
    else
	ok hidden "$(head -n 1 smoke.out | sed "s/^#'//; s/''/'/g" | cut -c 1-100)"
    fi
}

finish 67-sdl2
