#!/bin/sh
# 69-simdutf - the SimdUTFPlugin, built in, and its Smalltalk package
#
# build-wasm/node/pharo, then build-wasm/web/pharo-web.js, on copies of the
# stock image (plugins/SimdUTFPlugin, docs/SimdUTF.md):
#
#   builtin  Smalltalk vm listBuiltinModules has 'SimdUTFPlugin (i)';
#   load     plugins/SimdUTFPlugin/tests/load-simdutf.st loads the groups
#            Core and Tests of BaselineOfSimdUTF with Metacello from the
#            submodule plugins/SimdUTFPlugin/simdutf.st, and the image is
#            saved: the checks below run on copies of it;
#   info     SimdUTF pluginInfo is what primitiveInfo answers on the web:
#            protocol 1, the plugin 'SimdUTFPlugin 1.0', simdutf '9.4.0',
#            its kernel 'fallback' (simdutf has none for WebAssembly), the
#            platform 'emscripten', the kernels #('fallback'), and no
#            SIMDUTF_FORCE_IMPLEMENTATION; and SimdUTF backend is #plugin;
#   suite    tests/wasm/st/simdutf-lane.st runs the package SimdUTF-Tests
#            but SimdUTFSlowTests: no failure and no error, on the backend
#            #plugin;
#   big      a ByteString of 32 MiB (1 MiB of it U+00E9) to UTF-8 and back,
#            and 24 MiB of bytes to base64 and back, with old space at
#            1 GiB (PHARO_WASM_OLD_SPACE_BASE=0x40000000): results of 32 to
#            33 MiB, which the plugin may not have room for, so that the
#            image grows memory and calls again (OVER_BUDGET);
#   garbage  1000 conversions of 1 MiB in a row, whose results are dropped,
#            in the default window of old space (512 MiB): when the window is
#            full of their garbage, OVER_BUDGET cannot grow the memory and
#            the image collects the garbage and calls again (no OutOfMemory);
#   web      the suite again in pharo-web (the MEMORY64=2 module of the
#            pages), through packaging/emscripten/web/vm-driver.js and the
#            REPL of the Console page, as lane 68 runs SimdJSON's.
#
# Skipped when the CMake cache of the build turns FEATURE_PLUGIN_SIMDUTF off.
# SimdUTF is written for Pharo 12 to 15: every stock image runs the lane.
# Fails when the submodule is not checked out (git submodule update --init).
# The suite and the web run are each stopped after 600 s.
#
# Environment (from make wasm-check): NODE, WASM_DIR, SRCDIR, TEST_DIR, and
# WASM_CHECK_TIMEOUT (see lib/common.sh).

SRCDIR=$(cd "${SRCDIR:-$(dirname "$0")/../../..}" && pwd)
TEST_DIR=${TEST_DIR:+$TEST_DIR/69-simdutf}
# (the defaults of WASM_DIR, TEST_DIR and NODE, and the checks)
. "$SRCDIR/tests/wasm/lib/common.sh"
lane=simdutf
PLUGIN=$SRCDIR/plugins/SimdUTFPlugin
LANE_ST=$SRCDIR/tests/wasm/st/simdutf-lane.st
# (the VM of node reads the environment of the host: the kernel is the
# plugin's own choice here)
unset SIMDUTF_FORCE_IMPLEMENTATION

if ! grep -Eiq '^FEATURE_PLUGIN_SIMDUTF:BOOL=(ON|TRUE|YES|Y|1)$' "$WASM_DIR/cmake/CMakeCache.txt" 2>/dev/null; then
    skip "the build has no SimdUTFPlugin (FEATURE_PLUGIN_SIMDUTF is off)"
fi
for f in "$WASM_DIR/node/pharo" "$WASM_DIR/web/pharo-web.js" "$WASM_DIR/web/pharo-web.wasm"; do
    if ! test -f "$f"; then
	echo "FAIL 69-simdutf: no $f (make wasm)"
	exit 1
    fi
done
if ! test -d "$PLUGIN/simdutf.st/src/BaselineOfSimdUTF"; then
    echo "FAIL 69-simdutf: no $PLUGIN/simdutf.st/src (git submodule update --init plugins/SimdUTFPlugin/simdutf.st)"
    exit 1
fi
version=$(cat "$WASM_DIR/image/stock/pharo.version" 2>/dev/null)
echo "69-simdutf: $WASM_DIR/node/pharo, Pharo $version"

pharo_run() { limit "$timeout" "$WASM_DIR/node/pharo" "$@"; }
P="pharo_run --headless Pharo.image --no-default-preferences"
# (the image with SimdUTF and SimdUTF-Tests, which load saves)
loaded=$TEST_DIR/simdutf-image

load() (
    SU_PLUGIN_DIR=$PLUGIN SU_GROUPS='Core Tests'; export SU_PLUGIN_DIR SU_GROUPS
    $P st --save --quit "$PLUGIN/tests/load-simdutf.st"
)
suite() { limit 600 "$WASM_DIR/node/pharo" --headless Pharo.image --no-default-preferences st --quit "$LANE_ST"; }
big() (
    PHARO_WASM_OLD_SPACE_BASE=0x40000000; export PHARO_WASM_OLD_SPACE_BASE
    $P eval "| s e d b n t |
	s := String new: 32 * 1024 * 1024 withAll: \$a.
	s replaceFrom: 1 to: 1048576 with: (String new: 1048576 withAll: (Character value: 16rE9)) startingAt: 1.
	e := SimdUTF utf8Encode: s.
	d := SimdUTF utf8Decode: e.
	b := ByteArray new: 24 * 1024 * 1024.
	b replaceFrom: 1 to: 256 with: ((0 to: 255) asArray asByteArray) startingAt: 1.
	n := 256.
	[ n < b size ] whileTrue: [ b replaceFrom: n + 1 to: (2 * n min: b size) with: b startingAt: 1. n := 2 * n ].
	t := SimdUTF base64Encode: b.
	{ e size. d = s. d class name. t size. (SimdUTF base64Decode: t) = b }"
)
garbage() {
    $P eval "| s n |
	s := String new: 1048576 withAll: \$a.
	n := 0.
	1000 timesRepeat: [ n := n + (SimdUTF utf8Encode: s) size ].
	n"
}

# web: simdutf-lane.st evaluated in the REPL of the Console page, whose
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
      { path: "/pharo/simdutf-lane.st", data: fs.readFileSync(script) },
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
  drv.feed("Smalltalk compiler evaluate: \x27/pharo/simdutf-lane.st\x27 asFileReference contents\n");
  await prompt(at);
  process.stdout.write(out.slice(at));
  process.exit(0);
})().catch(e => { process.stdout.write(out.slice(-2000)); console.error("web: " + e.message); process.exit(1); });
' "$SRCDIR" "$WASM_DIR/web" "$loaded" "$LANE_ST"
}

# suite_passed NAME: the line of simdutf-lane.st in smoke.out, 'SimdUTF-Tests:
# <run> run, <failures> failures, <errors> errors, <skipped> skipped, backend
# #<backend>', has tests, no failure, and the backend #plugin (without the
# plugin its tests would skip themselves, and pass)
suite_passed() {
    summary=$(grep '^SimdUTF-Tests: ' smoke.out | head -n 1)
    set -- $summary
    if test "$status" -ne 0; then
	fail "$name" "exit status $status"
    elif test -z "$summary"; then
	fail "$name" "no 'SimdUTF-Tests:' line"
    elif ! is_number "$2" || test "$2" -eq 0; then
	fail "$name" "no test ran: '$summary'"
    elif test "x$4" != x0 || test "x$6" != x0; then
	fail "$name" "$summary"
    elif test "x${11}" != 'x#plugin'; then
	fail "$name" "not on the plugin: '$summary'"
    else
	ok "$name" "$2 tests, $8 skipped"
    fi
}

fresh builtin
expect builtin true $P eval "Smalltalk vm listBuiltinModules includes: 'SimdUTFPlugin (i)'"

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
    finish 69-simdutf
fi

fresh info "$loaded"
expect info "#(#(1 'SimdUTFPlugin 1.0' '9.4.0' 'fallback' 'emscripten' #('fallback') nil) #plugin)" $P eval \
    "{ SimdUTF pluginInfo. SimdUTF backend }"

fresh suite "$loaded"
if run suite suite; then
    suite_passed suite
fi

fresh big "$loaded"
expect big "#($((33 * 1024 * 1024)) true #ByteString $((32 * 1024 * 1024)) true)" big

fresh garbage "$loaded"
expect garbage $((1000 * 1048576)) garbage

fresh web "$loaded"
if run web web; then
    suite_passed web
fi

rm -rf "$loaded"
finish 69-simdutf
