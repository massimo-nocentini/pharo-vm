#!/bin/sh
# 68-simdjson - the SimdJSONPlugin, built in, and its Smalltalk library
#
# build-wasm/node/pharo, then build-wasm/web/pharo-web.js, on copies of the
# stock image (plugins/SimdJSONPlugin, docs/SimdJSON.md):
#
#   builtin  Smalltalk vm listBuiltinModules has 'SimdJSONPlugin (i)';
#   load     plugins/SimdJSONPlugin/st/SimdJSON.st, then SimdJSON-Tests.st,
#            filed in and saved: the checks below run on copies of that image;
#   info     SimdJSONReader is a subclass of SimdJSONMapper (SimdJSON.st
#            needs no NeoJSON), and the plugin answers its platform,
#            'emscripten', the implementation of simdjson, 'fallback'
#            (simdjson has no kernel for WebAssembly), and the version of
#            simdjson, '5.0.5';
#   suite    tests/wasm/st/simdjson-lane.st runs the package SimdJSON-Tests
#            but SimdJSONSlowTests: no failure and no error;
#   cap      a document of 17 MiB, one string, is refused with a
#            SimdJSONResourceError under the default cap of a value
#            (SimdJSONReader maxInputUnits, 16 Mi units), and read with no
#            cap, with old space at 1 GiB
#            (PHARO_WASM_OLD_SPACE_BASE=0x40000000, as docs/SimdJSON.md
#            advises for large documents);
#   web      the suite again in pharo-web (the MEMORY64=2 module of the
#            pages), through packaging/emscripten/web/vm-driver.js and the
#            REPL of the Console page, as lane 60 runs them.
#
# Skipped when the CMake cache of the build turns FEATURE_PLUGIN_SIMDJSON off,
# or when the stock image is older than Pharo 15, for which SimdJSON.st is
# written: build with WASM_IMAGE_ZIP=<a Pharo 15 zip>.  The suite and the web
# run are each stopped after 600 s.
#
# Environment (from make wasm-check): NODE, WASM_DIR, SRCDIR, TEST_DIR, and
# WASM_CHECK_TIMEOUT (see lib/common.sh).

SRCDIR=$(cd "${SRCDIR:-$(dirname "$0")/../../..}" && pwd)
TEST_DIR=${TEST_DIR:+$TEST_DIR/68-simdjson}
# (the defaults of WASM_DIR, TEST_DIR and NODE, and the checks)
. "$SRCDIR/tests/wasm/lib/common.sh"
lane=simdjson
ST=$SRCDIR/plugins/SimdJSONPlugin/st
LANE_ST=$SRCDIR/tests/wasm/st/simdjson-lane.st

if ! grep -Eiq '^FEATURE_PLUGIN_SIMDJSON:BOOL=(ON|TRUE|YES|Y|1)$' "$WASM_DIR/cmake/CMakeCache.txt" 2>/dev/null; then
    skip "the build has no SimdJSONPlugin (FEATURE_PLUGIN_SIMDJSON is off)"
fi
version=$(cat "$WASM_DIR/image/stock/pharo.version" 2>/dev/null)
if ! is_number "$version" || test "$version" -lt 150; then
    skip "the stock image is not Pharo 15 or later (pharo.version '$version'): build with WASM_IMAGE_ZIP=<a Pharo 15 zip>"
fi
for f in "$WASM_DIR/node/pharo" "$WASM_DIR/web/pharo-web.js" "$WASM_DIR/web/pharo-web.wasm"; do
    if ! test -f "$f"; then
	echo "FAIL 68-simdjson: no $f (make wasm)"
	exit 1
    fi
done
for f in "$ST/SimdJSON.st" "$ST/SimdJSON-Tests.st"; do
    if ! test -f "$f"; then
	echo "FAIL 68-simdjson: no $f"
	exit 1
    fi
done
echo "68-simdjson: $WASM_DIR/node/pharo, Pharo $version"

pharo_run() { limit "$timeout" "$WASM_DIR/node/pharo" "$@"; }
P="pharo_run --headless Pharo.image --no-default-preferences"
# (the image with SimdJSON.st and SimdJSON-Tests.st, which load saves)
loaded=$TEST_DIR/simdjson-image

load() { $P st --save --quit "$ST/SimdJSON.st" && $P st --save --quit "$ST/SimdJSON-Tests.st"; }
suite() { limit 600 "$WASM_DIR/node/pharo" --headless Pharo.image --no-default-preferences st --quit "$LANE_ST"; }
cap() (
    PHARO_WASM_OLD_SPACE_BASE=0x40000000; export PHARO_WASM_OLD_SPACE_BASE
    $P eval "| s capped | s := String new: 17 * 1024 * 1024 + 2 withAll: \$a. s at: 1 put: \$\"; at: s size put: \$\".
	capped := [ SimdJSONReader fromString: s. #read ] on: SimdJSONResourceError do: [ :e | #refused ].
	SimdJSONReader maxInputUnits: nil.
	{ capped. (SimdJSONReader fromString: s) size }"
)

# web: simdjson-lane.st evaluated in the REPL of the Console page, whose
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
      { path: "/pharo/simdjson-lane.st", data: fs.readFileSync(script) },
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
  drv.feed("Smalltalk compiler evaluate: \x27/pharo/simdjson-lane.st\x27 asFileReference contents\n");
  await prompt(at);
  process.stdout.write(out.slice(at));
  process.exit(0);
})().catch(e => { process.stdout.write(out.slice(-2000)); console.error("web: " + e.message); process.exit(1); });
' "$SRCDIR" "$WASM_DIR/web" "$loaded" "$LANE_ST"
}

# suite_passed NAME: the line of simdjson-lane.st in smoke.out, 'SimdJSON-Tests:
# <run> run, <failures> failures, <errors> errors', has tests and no failure
suite_passed() {
    summary=$(grep '^SimdJSON-Tests: ' smoke.out | head -n 1)
    set -- $summary
    if test "$status" -ne 0; then
	fail "$name" "exit status $status"
    elif test -z "$summary"; then
	fail "$name" "no 'SimdJSON-Tests:' line"
    elif ! is_number "$2" || test "$2" -eq 0; then
	fail "$name" "no test ran: '$summary'"
    elif test "x$4" != x0 || test "x$6" != x0; then
	fail "$name" "$summary"
    else
	ok "$name" "$2 tests"
    fi
}

fresh builtin
expect builtin true $P eval "Smalltalk vm listBuiltinModules includes: 'SimdJSONPlugin (i)'"

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
    finish 68-simdjson
fi

fresh info "$loaded"
expect info "#(#SimdJSONMapper true 'emscripten' 'fallback' '5.0.5')" $P eval \
    "{ SimdJSONReader superclass name. SimdJSONReader pluginAvailable. SimdJSONReader platform. SimdJSONReader implementationName. SimdJSONReader simdjsonVersion }"

fresh suite "$loaded"
if run suite suite; then
    suite_passed suite
fi

fresh cap "$loaded"
expect cap "#(#refused $((17 * 1024 * 1024)))" cap

fresh web "$loaded"
if run web web; then
    suite_passed web
fi

rm -rf "$loaded"
finish 68-simdjson
