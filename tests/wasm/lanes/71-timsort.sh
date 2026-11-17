#!/bin/sh
# 71-timsort - the TimSortPlugin, built in, and its Smalltalk package
#
# build-wasm/node/pharo, then build-wasm/web/pharo-web.js, on copies of the
# stock image (plugins/TimSortPlugin, docs/TimSort.md):
#
#   builtin   Smalltalk vm listBuiltinModules has 'TimSortPlugin (i)';
#   load      plugins/TimSortPlugin/tests/load-timsort.st loads the groups
#             Core, Tests and SmalltalkTests of BaselineOfTimSort with
#             Metacello from the submodule plugins/TimSortPlugin/timsort.st,
#             and the image is saved: the checks below run on copies of it;
#   info      TimSort pluginInfo is what primitiveInfo answers on the web:
#             protocol 1, the plugin 'TimSortPlugin 1.0', 'timsort.c
#             fd70381', the platform 'emscripten', no resumable sort (no
#             stack switching in WebAssembly without ASYNCIFY or JSPI) and
#             none live; TimSort backend is #plugin, its blockSortBackend
#             #smalltalk (the default on every VM), no resumableBackends,
#             and coroutinesAvailable false;
#   fallback  a sort by a sortBlock answers Pharo's result through the port
#             TimSortSmalltalk, while primitiveBlockSortNew fails with
#             #'unsupported operation', TimSort sort:with:backend: #plugin
#             signals TimSortBackendUnavailable, and no resumable sort is live;
#   suite     tests/wasm/st/timsort-lane.st runs the packages TimSort-Tests
#             and TimSort-Smalltalk-Tests: no failure and no error, on the
#             backend #plugin, with the sorts by a sortBlock on #smalltalk
#             and no resumable C sort;
#   big       an Array of 2^20 SmallIntegers, a Float64Array of 2^22
#             elements (32 MiB) and a ByteArray of 16 MiB sorted in the
#             default window of old space: the engine mallocs 128 MiB to widen
#             the bytes to 8-byte words, and timsort its merge buffers, in the
#             C heap of the module, which grows; each result must be sorted;
#   web       the suite again in pharo-web (the MEMORY64=2 module of the
#             pages), through packaging/emscripten/web/vm-driver.js and the
#             REPL of the Console page, as lane 69 runs SimdUTF's.
#
# Skipped when the CMake cache of the build turns FEATURE_PLUGIN_TIMSORT off.
# TimSort is written for Pharo 12 to 15: every stock image runs the lane.
# Fails when the submodule is not checked out (git submodule update --init).
# The suite, the big sorts and the web run are each stopped after 600 s.
#
# Environment (from make wasm-check): NODE, WASM_DIR, SRCDIR, TEST_DIR, and
# WASM_CHECK_TIMEOUT (see lib/common.sh).

SRCDIR=$(cd "${SRCDIR:-$(dirname "$0")/../../..}" && pwd)
TEST_DIR=${TEST_DIR:+$TEST_DIR/71-timsort}
# (the defaults of WASM_DIR, TEST_DIR and NODE, and the checks)
. "$SRCDIR/tests/wasm/lib/common.sh"
lane=timsort
PLUGIN=$SRCDIR/plugins/TimSortPlugin
LANE_ST=$SRCDIR/tests/wasm/st/timsort-lane.st

if ! grep -Eiq '^FEATURE_PLUGIN_TIMSORT:BOOL=(ON|TRUE|YES|Y|1)$' "$WASM_DIR/cmake/CMakeCache.txt" 2>/dev/null; then
    skip "the build has no TimSortPlugin (FEATURE_PLUGIN_TIMSORT is off)"
fi
for f in "$WASM_DIR/node/pharo" "$WASM_DIR/web/pharo-web.js" "$WASM_DIR/web/pharo-web.wasm"; do
    if ! test -f "$f"; then
	echo "FAIL 71-timsort: no $f (make wasm)"
	exit 1
    fi
done
if ! test -d "$PLUGIN/timsort.st/src/BaselineOfTimSort"; then
    echo "FAIL 71-timsort: no $PLUGIN/timsort.st/src (git submodule update --init plugins/TimSortPlugin/timsort.st)"
    exit 1
fi
version=$(cat "$WASM_DIR/image/stock/pharo.version" 2>/dev/null)
echo "71-timsort: $WASM_DIR/node/pharo, Pharo $version"

pharo_run() { limit "$timeout" "$WASM_DIR/node/pharo" "$@"; }
P="pharo_run --headless Pharo.image --no-default-preferences"
# (the image with TimSort, TimSort-Tests and TimSort-Smalltalk-Tests, which
# load saves)
loaded=$TEST_DIR/timsort-image

load() (
    TS_PLUGIN_DIR=$PLUGIN TS_GROUPS='Core Tests SmalltalkTests'; export TS_PLUGIN_DIR TS_GROUPS
    $P st --save --quit "$PLUGIN/tests/load-timsort.st"
)
suite() { limit 600 "$WASM_DIR/node/pharo" --headless Pharo.image --no-default-preferences st --quit "$LANE_ST"; }
big() {
    limit 600 "$WASM_DIR/node/pharo" --headless Pharo.image --no-default-preferences eval "| sorted a f b |
	sorted := [ :c | (1 to: c size - 1) allSatisfy: [ :i | (c at: i) <= (c at: i + 1) ] ].
	a := (1 to: 1048576) collect: [ :i | i * 7919 \\\\ 1048583 ].
	a timSort.
	f := Float64Array new: 4194304.
	1 to: f size do: [ :i | f at: i put: (i * 0.6180339887498949) fractionPart ].
	f timSort.
	b := ByteArray new: 16777216.
	1 to: b size do: [ :i | b at: i put: i * 7 \\\\ 251 ].
	b timSort.
	{ sorted value: a. sorted value: f. sorted value: b. b first. b last }"
}

# web: timsort-lane.st evaluated in the REPL of the Console page, whose
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
      { path: "/pharo/timsort-lane.st", data: fs.readFileSync(script) },
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
  drv.feed("Smalltalk compiler evaluate: \x27/pharo/timsort-lane.st\x27 asFileReference contents\n");
  await prompt(at);
  process.stdout.write(out.slice(at));
  process.exit(0);
})().catch(e => { process.stdout.write(out.slice(-2000)); console.error("web: " + e.message); process.exit(1); });
' "$SRCDIR" "$WASM_DIR/web" "$loaded" "$LANE_ST"
}

# suite_passed NAME: the line of timsort-lane.st in smoke.out, 'TimSort-Tests:
# <run> run, <failures> failures, <errors> errors, <skipped> skipped, backend
# #<backend>, blockSortBackend #<backend>, resumableBackend <backend>', has
# tests, no failure, the backend #plugin (without the plugin its tests would
# skip themselves, and pass), the sorts by a sortBlock on #smalltalk and no
# resumable C sort (nil)
suite_passed() {
    summary=$(grep '^TimSort-Tests: ' smoke.out | head -n 1)
    set -- $summary
    if test "$status" -ne 0; then
	fail "$name" "exit status $status"
    elif test -z "$summary"; then
	fail "$name" "no 'TimSort-Tests:' line"
    elif ! is_number "$2" || test "$2" -eq 0; then
	fail "$name" "no test ran: '$summary'"
    elif test "x$4" != x0 || test "x$6" != x0; then
	fail "$name" "$summary"
    elif test "x${11}" != 'x#plugin,' || test "x${13}" != 'x#smalltalk,' || test "x${15}" != 'xnil'; then
	fail "$name" "not on the plugin, with the sorts by a sortBlock in Smalltalk and no resumable sort: '$summary'"
    else
	ok "$name" "$2 tests, $8 skipped"
    fi
}

fresh builtin
expect builtin true $P eval "Smalltalk vm listBuiltinModules includes: 'TimSortPlugin (i)'"

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
    finish 71-timsort
fi

fresh info "$loaded"
expect info "#(#(1 'TimSortPlugin 1.0' 'timsort.c fd70381' 'emscripten' false 0) #plugin #smalltalk #() false)" $P eval \
    "{ TimSort pluginInfo. TimSort backend. TimSort blockSortBackend. TimSort resumableBackends. TimSort coroutinesAvailable }"

fresh fallback "$loaded"
expect fallback "#(#(9 7 5 3 3 1) #'unsupported operation' #plugin 0)" $P eval \
    "{ #(5 3 9 1 7 3) copy timSort: [ :a :b | a >= b ]. (TimSortPrimitives blockSortNew: 6 descending: false) ec. [ TimSort sort: #(2 1) copy with: [ :a :b | a <= b ] backend: #plugin ] on: TimSortBackendUnavailable do: [ :e | e return: e backendName ]. TimSort liveCoroutines }"

fresh suite "$loaded"
if run suite suite; then
    suite_passed suite
fi

fresh big "$loaded"
expect big "#(true true true 0 250)" big

fresh web "$loaded"
if run web web; then
    suite_passed web
fi

rm -rf "$loaded"
finish 71-timsort
