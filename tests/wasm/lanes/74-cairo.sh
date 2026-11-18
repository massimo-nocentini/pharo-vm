#!/bin/sh
# 74-cairo - the CairoPlugin, built in, and its Smalltalk packages
#
# build-wasm/node/pharo, then build-wasm/web/pharo-web.js, on copies of the
# prepared image of the world, image/web, whose OSWindow-Web lets Athens make
# a surface without the SurfacePlugin (as lane 65); a build without it uses
# the stock image with plugins/CairoPlugin/tests/web-surface.st filed in
# (plugins/CairoPlugin, docs/CairoPlugin.md):
#
#   builtin   Smalltalk vm listBuiltinModules has 'CairoPlugin (i)';
#   surface   on the stock image, web-surface.st filed in and saved: the
#             methods of AthensCairoSurface it compiles have the sources of
#             those of packaging/emscripten/st/OSWindow-Web/
#             AthensCairoSurface.extension.st (TonelParser), so that the
#             fallback image draws as image/web does;
#   probe     plugins/CairoPlugin/tests/ci-load.st, the probe of the CI jobs:
#             binds the plugin to the cairo of the FFI's registry, draws its
#             scene with the same pixels by the FFI, by the one-for-one
#             primitives and by the shape and text primitives, and prints
#             'CairoPlugin emscripten 1 cairo <x.y.z> ok';
#   load      plugins/CairoPlugin/tests/load-cairo.st loads the groups Cairo
#             and CairoTests of BaselineOfVMPluginsPrimitives with Metacello
#             from the submodule plugins/UtilsPlugin/vm-plugins-primitives.st
#             (or CP_ST_REPO), and the image is saved: the checks below run on
#             copies of it, started again, so CairoPlugin startUp: binds;
#   info      CairoPlugin info is what primitiveInfo answers on the web:
#             protocol 1, 'CairoPlugin 1.0', 'emscripten', bound to the
#             version of cairo the probe printed, 'static', no missing
#             function, no missing optional one;
#   suite     tests/wasm/st/cairo-lane.st, with CP_SCENE_SCALE=0.05 and
#             CP_SCENE_SIZE=250: the plugin bound and the groups #basic
#             #shape #text installed, then the package
#             VM-Plugins-Primitives-Cairo-Tests with no failure, no error
#             and no test skipped (each skip is printed with its reason; the
#             tests of a VM without the plugin are 'not applicable', not
#             skips);
#   web       the same in pharo-web (the MEMORY64=2 module of the pages),
#             through packaging/emscripten/web/vm-driver.js and the REPL of
#             the Console page, as lane 73, with the empty placeholder
#             /pharo/libcairo.so.2 in its files (which vm-worker.js writes
#             for the pages, and vm-driver.js does not), and only
#             CairoPluginParityTest and CairoPluginPrimitiveTest.
#
# Skipped when the CMake cache of the build turns FEATURE_PLUGIN_CAIRO off,
# or says the plugin was left out (PHARO_CAIRO_PLUGIN_BUILT OFF: a build
# without pharo_cairo, WASM_CAIRO=OFF, whatever the cached option says).
# Fails when the submodule is not checked out (git submodule update --init),
# and when it is too old to have the packages of the CairoPlugin
# (src/VM-Plugins-Primitives-Cairo).  CP_ST_REPO, the src directory of a
# clone of vm-plugins-primitives.st, replaces the submodule (to try the
# Smalltalk code before the gitlink is bumped).  The load, the suite and the
# web run are each stopped after 600 s; the lane prints its time.
#
# Environment (from make wasm-check): NODE, WASM_DIR, SRCDIR, TEST_DIR, and
# WASM_CHECK_TIMEOUT (see lib/common.sh); CP_ST_REPO.

SRCDIR=$(cd "${SRCDIR:-$(dirname "$0")/../../..}" && pwd)
TEST_DIR=${TEST_DIR:+$TEST_DIR/74-cairo}
# (the defaults of WASM_DIR, TEST_DIR and NODE, and the checks)
. "$SRCDIR/tests/wasm/lib/common.sh"
lane=cairo-plugin
started=$(date +%s)
PLUGIN=$SRCDIR/plugins/CairoPlugin
SUBMODULE=$SRCDIR/plugins/UtilsPlugin/vm-plugins-primitives.st
LANE_ST=$SRCDIR/tests/wasm/st/cairo-lane.st
WEB_SURFACE=$PLUGIN/tests/web-surface.st
OSWEB_EXTENSION=$SRCDIR/packaging/emscripten/st/OSWindow-Web/AthensCairoSurface.extension.st
cache=$WASM_DIR/cmake/CMakeCache.txt

if ! grep -Eiq '^FEATURE_PLUGIN_CAIRO:BOOL=(ON|TRUE|YES|Y|1)$' "$cache" 2>/dev/null; then
    skip "the build has no CairoPlugin (FEATURE_PLUGIN_CAIRO is off)"
fi
if grep -Eiq '^PHARO_CAIRO_PLUGIN_BUILT:INTERNAL=(OFF|FALSE|NO|N|0)$' "$cache" 2>/dev/null; then
    skip "the build left the CairoPlugin out (no pharo_cairo: WASM_CAIRO=OFF)"
fi
for f in "$WASM_DIR/node/pharo" "$WASM_DIR/web/pharo-web.js" "$WASM_DIR/web/pharo-web.wasm" \
    "$PLUGIN/tests/ci-load.st" "$LANE_ST" "$OSWEB_EXTENSION"; do
    if ! test -f "$f"; then
	echo "FAIL 74-cairo: no $f"
	exit 1
    fi
done
if test -n "$CP_ST_REPO"; then
    repository=$(cd "$CP_ST_REPO" 2>/dev/null && pwd)
    if test -z "$repository" || ! test -d "$repository/BaselineOfVMPluginsPrimitives"; then
	echo "FAIL 74-cairo: CP_ST_REPO=$CP_ST_REPO is not the src directory of vm-plugins-primitives.st"
	exit 1
    fi
    if ! test -d "$repository/VM-Plugins-Primitives-Cairo"; then
	echo "FAIL 74-cairo: CP_ST_REPO=$CP_ST_REPO has no VM-Plugins-Primitives-Cairo"
	exit 1
    fi
else
    repository=$SUBMODULE/src
    if ! test -d "$repository/BaselineOfVMPluginsPrimitives"; then
	echo "FAIL 74-cairo: no $repository (git submodule update --init plugins/UtilsPlugin/vm-plugins-primitives.st)"
	exit 1
    fi
    if ! test -d "$repository/VM-Plugins-Primitives-Cairo"; then
	echo "FAIL 74-cairo: submodule too old: bump plugins/UtilsPlugin/vm-plugins-primitives.st to the commit with the Cairo packages (no $repository/VM-Plugins-Primitives-Cairo)"
	exit 1
    fi
fi
version=$(cat "$WASM_DIR/image/stock/pharo.version" 2>/dev/null)
echo "74-cairo: $WASM_DIR/node/pharo, Pharo $version, Smalltalk code from $repository"

pharo_run() { limit "$timeout" "$WASM_DIR/node/pharo" "$@"; }
P="pharo_run --headless Pharo.image --no-default-preferences"
# (the base image: image/web, or the stock one with web-surface.st, which
# the check surface saves; and the image with the packages, which load saves)
base=$TEST_DIR/cairo-base
loaded=$TEST_DIR/cairo-image
# (the comparison of the check surface, run on the image it saved)
surface_st=$TEST_DIR/cairo-surface.st
cat >"$surface_st" <<'EOF'
"The methods of AthensCairoSurface (and its class) of the Tonel file CP_OSWEB_EXTENSION against
those of the image: prints 'surface: <n> methods, differ <those whose source differs or are not
there>'."
| lineEnd normal definitions differ |
lineEnd := OSPlatform current lineEnding.
normal := [ :source | ((source copyReplaceAll: String crlf with: String cr) copyReplaceAll: String lf with: String cr) trimBoth ].
definitions := (TonelParser parseString: (OSEnvironment current at: 'CP_OSWEB_EXTENSION') asFileReference contents)
	select: [ :each | each isMethodDefinition ].
differ := definitions reject: [ :each | | class method |
	class := Smalltalk globals at: each className asSymbol ifAbsent: [ nil ].
	class := class ifNotNil: [ each classIsMeta ifTrue: [ class class ] ifFalse: [ class ] ].
	method := class ifNotNil: [ class compiledMethodAt: each selector asSymbol ifAbsent: [ nil ] ].
	method notNil and: [ (normal value: method sourceCode) = (normal value: each source) ] ].
Stdio stdout
	nextPutAll: 'surface: ';
	nextPutAll: definitions size printString;
	nextPutAll: ' methods, differ ';
	nextPutAll: (differ collect: [ :each |
		each className asString , (each classIsMeta ifTrue: [ ' class>>' ] ifFalse: [ '>>' ]) , each selector asString ]) asArray printString;
	nextPutAll: lineEnd;
	flush.
Smalltalk snapshot: false andQuit: true.
EOF

surface() {
    $P st --save --quit "$WEB_SURFACE" || return
    CP_OSWEB_EXTENSION=$OSWEB_EXTENSION pharo_run --headless Pharo.image --no-default-preferences st "$surface_st"
}
probe() { $P st "$PLUGIN/tests/ci-load.st"; }
load() (
    CP_PLUGIN_DIR=$SRCDIR/plugins/UtilsPlugin CP_GROUPS='Cairo CairoTests' CP_ST_REPO=$repository
    export CP_PLUGIN_DIR CP_GROUPS CP_ST_REPO
    limit 600 "$WASM_DIR/node/pharo" --headless Pharo.image --no-default-preferences st --save --quit "$PLUGIN/tests/load-cairo.st"
)
suite() (
    CP_SCENE_SCALE=0.05 CP_SCENE_SIZE=250; export CP_SCENE_SCALE CP_SCENE_SIZE
    limit 600 "$WASM_DIR/node/pharo" --headless Pharo.image --no-default-preferences st --quit "$LANE_ST"
)

# web: cairo-lane.st evaluated in the REPL of the Console page, whose
# output from there to the next prompt it prints
web() {
    limit 600 "$NODE" -e '
const fs = require("fs"), path = require("path");
const [srcDir, webDir, imageDir, script] = process.argv.slice(1);
const createPharoVM = require(path.join(webDir, "pharo-web.js"));
const Driver = require(path.join(srcDir, "packaging", "emscripten", "web", "vm-driver.js"));
const only = ext => fs.readdirSync(imageDir).filter(f => f.endsWith(ext))[0];
const image = only(".image"), sources = only(".sources");
const dec = { 1: new TextDecoder(), 2: new TextDecoder() };
let out = "", exit = null, crash = null, waiters = [];
const notify = () => { const w = waiters; waiters = []; w.forEach(f => f()); };
const prompt = from => new Promise((resolve, reject) => {
  const check = () => {
    if (crash !== null) return reject(new Error("crash: " + crash));
    if (exit !== null) return reject(new Error("exit " + exit));
    if (out.length > from && out.endsWith("st> ")) return resolve();
    waiters.push(check);
  };
  check();
});
(async () => {
  const drv = await Driver.start(createPharoVM, {
    args: Driver.vmArgs("console"),
    env: { CP_SCENE_SCALE: "0.05", CP_SCENE_SIZE: "250", CP_LANE_CLASSES: "CairoPluginParityTest CairoPluginPrimitiveTest" },
    files: [
      { path: "/pharo/Pharo.image", data: fs.readFileSync(path.join(imageDir, image)) },
      { path: "/pharo/Pharo.changes", data: fs.readFileSync(path.join(imageDir, image.replace(/\.image$/, ".changes"))) },
      { path: "/pharo/" + sources, data: fs.readFileSync(path.join(imageDir, sources)) },
      { path: "/pharo/st/web-repl.st", data: fs.readFileSync(path.join(srcDir, "packaging", "emscripten", "st", "web-repl.st")) },
      { path: "/pharo/cairo-lane.st", data: fs.readFileSync(script) },
      // (the library of CairoLibrary, as vm-worker.js writes it for the pages: the image finds
      // cairo only as a file of the VM directory, then binds the one of the registry)
      { path: "/pharo/libcairo.so.2", data: new Uint8Array(0) },
    ],
    schedule: f => setImmediate(f),
    later: (f, ms) => setTimeout(f, ms),
    cancel: h => clearTimeout(h),
    canRun: () => true,
    onOutput: (fd, bytes) => {
      const text = dec[fd].decode(bytes, { stream: true });
      out += text;
      if (fd === 2) process.stderr.write(text);
      notify();
    },
    onState: notify,
    onHost: () => {},
    onExit: code => { exit = code; notify(); },
    onCrash: message => { crash = message; notify(); },
    onDiag: () => {},
  });
  drv.begin();
  await prompt(0);
  const at = out.length;
  drv.feed("Smalltalk compiler evaluate: \x27/pharo/cairo-lane.st\x27 asFileReference contents\n");
  await prompt(at);
  process.stdout.write(out.slice(at));
  process.exit(0);
})().catch(e => { process.stdout.write(out.slice(-2000)); console.error("web: " + e.message); process.exit(1); });
' "$SRCDIR" "$WASM_DIR/web" "$loaded" "$LANE_ST"
}

# suite_passed NAME: the lines of cairo-lane.st in smoke.out: 'CairoPlugin:
# bound true, installed #(#basic #shape #text), ...', then
# 'VM-Plugins-Primitives-Cairo-Tests: <run> run, <failures> failures,
# <errors> errors, <skipped> skipped, ...' with tests, no failure, no error
# and none skipped
suite_passed() {
    plugin=$(grep '^CairoPlugin: ' smoke.out | head -n 1)
    summary=$(grep '^VM-Plugins-Primitives-Cairo-Tests: ' smoke.out | head -n 1)
    set -- $summary
    if test "$status" -ne 0; then
	fail "$name" "exit status $status"
    elif test -z "$plugin"; then
	fail "$name" "no 'CairoPlugin:' line"
    else
	case $plugin in
	    'CairoPlugin: bound true, installed #(#basic #shape #text),'*)
		if test -z "$summary"; then
		    fail "$name" "no 'VM-Plugins-Primitives-Cairo-Tests:' line"
		elif ! is_number "$2" || test "$2" -eq 0; then
		    fail "$name" "no test ran: '$summary'"
		elif test "x$4" != x0 || test "x$6" != x0 || test "x$8" != x0; then
		    fail "$name" "$summary"
		else
		    ok "$name" "$2 tests"
		fi
		;;
	    *) fail "$name" "the plugin is not bound with every group installed: '$plugin'" ;;
	esac
    fi
}

fresh builtin
expect builtin true $P eval "Smalltalk vm listBuiltinModules includes: 'CairoPlugin (i)'"

# (without image/web, the base is the image the check surface saves)
web_image=$WASM_DIR/image/web
if ls "$web_image"/*.image >/dev/null 2>&1; then
    base=$web_image
else
    web_image=
    rm -rf "$base"
fi
fresh surface
if ! test -f "$WEB_SURFACE"; then
    checks=$((checks + 1))
    fail surface "no $WEB_SURFACE"
else
    if run surface surface; then
	if test "$status" -ne 0; then
	    fail surface "exit status $status"
	elif ! grep -q '^surface: [1-9][0-9]* methods, differ #()$' smoke.out; then
	    fail surface "the methods of web-surface.st differ from ${OSWEB_EXTENSION#$SRCDIR/}: $(grep '^surface: ' smoke.out | head -n 1)"
	else
	    ok surface "$(sed -n 's/^surface: \([0-9]*\) methods.*/\1 methods/p' smoke.out)"
	    if test -z "$web_image"; then
		if ! mkdir -p "$base" || ! cp -P Pharo.image Pharo.changes ./*.sources "$base/"; then
		    fail surface "cannot keep the image in $base"
		fi
	    fi
	fi
    fi
fi
if ! ls "$base"/*.image >/dev/null 2>&1; then
    echo "74-cairo: no image/web and no stock image with web-surface.st: nothing to run on"
    finish 74-cairo
fi
echo "74-cairo: on ${web_image:+the image of the world, }$base"

fresh probe "$base"
cairo=
if run probe probe; then
    line=$(grep -E '^CairoPlugin emscripten 1 cairo [0-9]+\.[0-9]+\.[0-9]+ ok$' smoke.out | head -n 1)
    if test "$status" -ne 0; then
	fail probe "exit status $status: $(grep '^CairoPlugin' smoke.out | head -n 1)"
    elif test -z "$line"; then
	fail probe "no 'CairoPlugin emscripten 1 cairo <x.y.z> ok': $(grep '^CairoPlugin' smoke.out | head -n 1)"
    else
	cairo=$(echo "$line" | sed 's/^CairoPlugin emscripten 1 cairo \([0-9.]*\) ok$/\1/')
	ok probe "cairo $cairo; $(grep '^CairoPlugin scene ' smoke.out | sed 's/^CairoPlugin scene //' | tr '\n' ';' | sed 's/;$//; s/;/; /g')"
    fi
fi

fresh load "$base"
if ! test -f "$PLUGIN/tests/load-cairo.st"; then
    checks=$((checks + 1))
    fail load "no $PLUGIN/tests/load-cairo.st"
elif run load load; then
    if test "$status" -ne 0; then
	fail load "exit status $status: $(grep -v '^ *$' smoke.err | head -n 1)"
    elif ! rm -rf "$loaded" || ! mkdir -p "$loaded" || ! cp -P Pharo.image Pharo.changes ./*.sources "$loaded/"; then
	fail load "cannot keep the image in $loaded"
    else
	ok load "$(grep '^CairoPlugin: ' smoke.out | head -n 1)"
    fi
fi
if ! test -f "$loaded/Pharo.image"; then
    rm -rf "$TEST_DIR/cairo-base"
    finish 74-cairo
fi

# (the version of the probe, x.y.z, as cairo_version answers it: 10000 x + 100 y + z)
if test -n "$cairo"; then
    number=$(echo "$cairo" | awk -F. '{ print $1 * 10000 + $2 * 100 + $3 }')
else
    number='[0-9]*'
fi
fresh info "$loaded"
info_regex="^#\\(1 'CairoPlugin 1\\.0' 'emscripten' $number 'static' nil #\\(\\)\\)\$"
if run info $P eval "CairoPlugin info"; then
    if test "$status" -ne 0; then
	fail info "exit status $status"
    elif ! grep -Eq "$info_regex" smoke.out; then
	fail info "expected #(1 'CairoPlugin 1.0' 'emscripten' ${cairo:+$number }'static' nil #()), got '$(head -n 1 smoke.out)'"
    else
	ok info "$(head -n 1 smoke.out)"
    fi
fi

fresh suite "$loaded"
if run suite suite; then
    suite_passed suite
fi

fresh web "$loaded"
if run web web; then
    suite_passed web
fi

rm -rf "$loaded" "$TEST_DIR/cairo-base" "$surface_st"
echo "74-cairo: $(($(date +%s) - started)) s"
finish 74-cairo
