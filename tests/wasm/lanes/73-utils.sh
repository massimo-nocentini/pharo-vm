#!/bin/sh
# 73-utils - the UtilsPlugin, built in, and its Smalltalk package
#
# build-wasm/node/pharo, then build-wasm/web/pharo-web.js, on copies of the
# stock image (plugins/UtilsPlugin, docs/UtilsPlugin.md):
#
#   builtin   Smalltalk vm listBuiltinModules has 'UtilsPlugin (i)';
#   probe     plugins/UtilsPlugin/tests/ci-load.st, the probe of the CI jobs,
#             on the stock image: every primitive answers its known value,
#             Knuth's own check of his rng-double.c included, and prints
#             'UtilsPlugin emscripten 1 ok';
#   load      plugins/UtilsPlugin/tests/load-utils.st loads the groups Core
#             and Tests of BaselineOfVMPluginsPrimitives with Metacello from
#             the submodule plugins/UtilsPlugin/vm-plugins-primitives.st, and
#             the image is saved: the checks below run on copies of it;
#   info      UtilsPlugin info is what primitive_info answers on the web:
#             protocol 1, 'UtilsPlugin 1.0', 'emscripten';
#   suite     tests/wasm/st/utils-lane.st runs the package
#             VM-Plugins-Primitives-Tests: no failure, no error and no test
#             skipped (the tests of the plugin skip themselves without it);
#   web       the suite again in pharo-web (the MEMORY64=2 module of the
#             pages), through packaging/emscripten/web/vm-driver.js and the
#             REPL of the Console page, as lane 71 runs TimSort's.
#
# Skipped when the CMake cache of the build turns FEATURE_PLUGIN_UTILS off.
# VM-Plugins-Primitives is written for Pharo 12 to 15: every stock image runs
# the lane.  Fails when the submodule is not checked out (git submodule
# update --init).  The suite and the web run are each stopped after 600 s.
#
# Environment (from make wasm-check): NODE, WASM_DIR, SRCDIR, TEST_DIR, and
# WASM_CHECK_TIMEOUT (see lib/common.sh).

SRCDIR=$(cd "${SRCDIR:-$(dirname "$0")/../../..}" && pwd)
TEST_DIR=${TEST_DIR:+$TEST_DIR/73-utils}
# (the defaults of WASM_DIR, TEST_DIR and NODE, and the checks)
. "$SRCDIR/tests/wasm/lib/common.sh"
lane=utils
PLUGIN=$SRCDIR/plugins/UtilsPlugin
LANE_ST=$SRCDIR/tests/wasm/st/utils-lane.st

if ! grep -Eiq '^FEATURE_PLUGIN_UTILS:BOOL=(ON|TRUE|YES|Y|1)$' "$WASM_DIR/cmake/CMakeCache.txt" 2>/dev/null; then
    skip "the build has no UtilsPlugin (FEATURE_PLUGIN_UTILS is off)"
fi
for f in "$WASM_DIR/node/pharo" "$WASM_DIR/web/pharo-web.js" "$WASM_DIR/web/pharo-web.wasm"; do
    if ! test -f "$f"; then
	echo "FAIL 73-utils: no $f (make wasm)"
	exit 1
    fi
done
if ! test -d "$PLUGIN/vm-plugins-primitives.st/src/BaselineOfVMPluginsPrimitives"; then
    echo "FAIL 73-utils: no $PLUGIN/vm-plugins-primitives.st/src (git submodule update --init plugins/UtilsPlugin/vm-plugins-primitives.st)"
    exit 1
fi
version=$(cat "$WASM_DIR/image/stock/pharo.version" 2>/dev/null)
echo "73-utils: $WASM_DIR/node/pharo, Pharo $version"

pharo_run() { limit "$timeout" "$WASM_DIR/node/pharo" "$@"; }
P="pharo_run --headless Pharo.image --no-default-preferences"
# (the image with VM-Plugins-Primitives and its tests, which load saves)
loaded=$TEST_DIR/utils-image

probe() { $P st "$PLUGIN/tests/ci-load.st"; }
load() (
    UP_PLUGIN_DIR=$PLUGIN UP_GROUPS='Core Tests'; export UP_PLUGIN_DIR UP_GROUPS
    $P st --save --quit "$PLUGIN/tests/load-utils.st"
)
suite() { limit 600 "$WASM_DIR/node/pharo" --headless Pharo.image --no-default-preferences st --quit "$LANE_ST"; }

# web: utils-lane.st evaluated in the REPL of the Console page, whose
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
    files: [
      { path: "/pharo/Pharo.image", data: fs.readFileSync(path.join(imageDir, image)) },
      { path: "/pharo/Pharo.changes", data: fs.readFileSync(path.join(imageDir, image.replace(/\.image$/, ".changes"))) },
      { path: "/pharo/" + sources, data: fs.readFileSync(path.join(imageDir, sources)) },
      { path: "/pharo/st/web-repl.st", data: fs.readFileSync(path.join(srcDir, "packaging", "emscripten", "st", "web-repl.st")) },
      { path: "/pharo/utils-lane.st", data: fs.readFileSync(script) },
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
  drv.feed("Smalltalk compiler evaluate: \x27/pharo/utils-lane.st\x27 asFileReference contents\n");
  await prompt(at);
  process.stdout.write(out.slice(at));
  process.exit(0);
})().catch(e => { process.stdout.write(out.slice(-2000)); console.error("web: " + e.message); process.exit(1); });
' "$SRCDIR" "$WASM_DIR/web" "$loaded" "$LANE_ST"
}

# suite_passed NAME: the line of utils-lane.st in smoke.out,
# 'VM-Plugins-Primitives-Tests: <run> run, <failures> failures, <errors>
# errors, <skipped> skipped, plugin <info>', has tests, no failure, no error,
# and none skipped (without the plugin its tests would skip themselves, and
# pass)
suite_passed() {
    summary=$(grep '^VM-Plugins-Primitives-Tests: ' smoke.out | head -n 1)
    set -- $summary
    if test "$status" -ne 0; then
	fail "$name" "exit status $status"
    elif test -z "$summary"; then
	fail "$name" "no 'VM-Plugins-Primitives-Tests:' line"
    elif ! is_number "$2" || test "$2" -eq 0; then
	fail "$name" "no test ran: '$summary'"
    elif test "x$4" != x0 || test "x$6" != x0 || test "x$8" != x0; then
	fail "$name" "$summary"
    else
	ok "$name" "$2 tests"
    fi
}

fresh builtin
expect builtin true $P eval "Smalltalk vm listBuiltinModules includes: 'UtilsPlugin (i)'"

fresh probe
if run probe probe; then
    if test "$status" -ne 0; then
	fail probe "exit status $status: $(grep '^UtilsPlugin' smoke.out | head -n 1)"
    elif ! grep '^UtilsPlugin emscripten 1 ok' smoke.out >/dev/null; then
	fail probe "no 'UtilsPlugin emscripten 1 ok'"
    else
	ok probe
    fi
fi

fresh load
if run load load; then
    if test "$status" -ne 0; then
	fail load "exit status $status"
    elif ! rm -rf "$loaded" || ! mkdir -p "$loaded" || ! cp -P Pharo.image Pharo.changes ./*.sources "$loaded/"; then
	fail load "cannot keep the image in $loaded"
    else
	ok load
    fi
fi
if ! test -f "$loaded/Pharo.image"; then
    finish 73-utils
fi

fresh info "$loaded"
expect info "#(1 'UtilsPlugin 1.0' 'emscripten')" $P eval "UtilsPlugin info"

fresh suite "$loaded"
if run suite suite; then
    suite_passed suite
fi

fresh web "$loaded"
if run web web; then
    suite_passed web
fi

rm -rf "$loaded"
finish 73-utils
