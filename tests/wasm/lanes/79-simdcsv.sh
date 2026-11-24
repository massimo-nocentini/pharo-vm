#!/bin/sh
# 79-simdcsv - the SimdCSVPlugin, built in, and its Smalltalk package
#
# build-wasm/node/pharo, then build-wasm/web/pharo-web.js, on copies of the
# stock image (plugins/SimdCSVPlugin, docs/SimdCSV.md):
#
#   builtin  Smalltalk vm listBuiltinModules has 'SimdCSVPlugin (i)';
#   probe    plugins/SimdCSVPlugin/tests/ci-load.st, the probe of the CI jobs,
#            on the stock image: the known answers of the four primitives,
#            and 'SimdCSVPlugin emscripten fallback a9494a8 1 forced - ok';
#            then, with SIMDCSV_FORCE_IMPLEMENTATION=avx2, which this build
#            has not, 'forced refused avx2 ok';
#   load     plugins/SimdCSVPlugin/tests/load-simdcsv.st loads the groups
#            Core and Tests of BaselineOfSimdCSV with Metacello from the
#            submodule plugins/SimdCSVPlugin/simdcsv.st, finds the plugin, and
#            the image is saved: the checks below run on copies of it;
#   info     SimdCSV pluginInfo is what primitiveInfo answers on the web:
#            protocol 1, the plugin 'SimdCSVPlugin 1.0', simdcsv 'a9494a8',
#            the kernel 'fallback' (ours: there is no simdcsv code in the
#            module), the platform 'emscripten', the kernels #('fallback'),
#            no SIMDCSV_FORCE_IMPLEMENTATION, and the longest span of an
#            index, 2^32 - 2 units; and SimdCSV backend is #plugin;
#   suite    tests/wasm/st/simdcsv-lane.st runs the package SimdCSV-Tests but
#            SimdCSVSlowTest: no failure, no error and no test skipped, on
#            the backend #plugin;
#   big      a ByteArray of 32 MiB, with old space at 1 GiB
#            (PHARO_WASM_OLD_SPACE_BASE=0x40000000): its records counted, and
#            the offsets of its 3 Mi fields (a WordArray of 12 MiB);
#   garbage  200 times the rows of 1 MiB (65536 rows of 3 Strings, some 5 MiB
#            of objects), which are dropped, in the default window of old
#            space (512 MiB): old space is no larger after the 200 than after
#            the first 20;
#   web      the suite again in pharo-web (the MEMORY64=2 module of the
#            pages), through packaging/emscripten/web/vm-driver.js and the
#            REPL of the Console page, as lane 69 runs SimdUTF's.
#
# Skipped when the CMake cache of the build turns FEATURE_PLUGIN_SIMDCSV off.
# SimdCSV is written for Pharo 12 to 15: every stock image runs the lane.
# Fails when the submodule is not checked out (git submodule update --init
# plugins/SimdCSVPlugin/simdcsv.st).  The load, the suite and the web run are
# each stopped after 600 s; the lane prints its time.
#
# Environment (from make wasm-check): NODE, WASM_DIR, SRCDIR, TEST_DIR, and
# WASM_CHECK_TIMEOUT (see lib/common.sh).

SRCDIR=$(cd "${SRCDIR:-$(dirname "$0")/../../..}" && pwd)
TEST_DIR=${TEST_DIR:+$TEST_DIR/79-simdcsv}
# (the defaults of WASM_DIR, TEST_DIR and NODE, and the checks)
. "$SRCDIR/tests/wasm/lib/common.sh"
lane=simdcsv
started=$(date +%s)
PLUGIN=$SRCDIR/plugins/SimdCSVPlugin
LANE_ST=$SRCDIR/tests/wasm/st/simdcsv-lane.st
# (the VM of node reads the environment of the host: the kernel is the
# plugin's own choice here, and the slow tests stay out)
unset SIMDCSV_FORCE_IMPLEMENTATION SIMDCSV_SLOW_TESTS

if ! grep -Eiq '^FEATURE_PLUGIN_SIMDCSV:BOOL=(ON|TRUE|YES|Y|1)$' "$WASM_DIR/cmake/CMakeCache.txt" 2>/dev/null; then
    skip "the build has no SimdCSVPlugin (FEATURE_PLUGIN_SIMDCSV is off)"
fi
for f in "$WASM_DIR/node/pharo" "$WASM_DIR/web/pharo-web.js" "$WASM_DIR/web/pharo-web.wasm" \
    "$PLUGIN/tests/ci-load.st" "$PLUGIN/tests/load-simdcsv.st" "$LANE_ST"; do
    if ! test -f "$f"; then
	echo "FAIL 79-simdcsv: no $f"
	exit 1
    fi
done
if ! test -d "$PLUGIN/simdcsv.st/src/BaselineOfSimdCSV"; then
    echo "FAIL 79-simdcsv: no $PLUGIN/simdcsv.st/src (git submodule update --init plugins/SimdCSVPlugin/simdcsv.st)"
    exit 1
fi
version=$(cat "$WASM_DIR/image/stock/pharo.version" 2>/dev/null)
echo "79-simdcsv: $WASM_DIR/node/pharo, Pharo $version"

pharo_run() { limit "$timeout" "$WASM_DIR/node/pharo" "$@"; }
P="pharo_run --headless Pharo.image --no-default-preferences"
# (the image with SimdCSV and SimdCSV-Tests, which load saves)
loaded=$TEST_DIR/simdcsv-image

probe() { $P st "$PLUGIN/tests/ci-load.st"; }
probe_avx2() (
    SIMDCSV_FORCE_IMPLEMENTATION=avx2; export SIMDCSV_FORCE_IMPLEMENTATION
    probe
)
load() (
    SIMDCSV_PLUGIN_DIR=$PLUGIN SIMDCSV_GROUPS='Core Tests' SIMDCSV_EXPECT_PLUGIN=yes
    export SIMDCSV_PLUGIN_DIR SIMDCSV_GROUPS SIMDCSV_EXPECT_PLUGIN
    unset SIMDCSV_ST_DIR
    limit 600 "$WASM_DIR/node/pharo" --headless Pharo.image --no-default-preferences \
	st --save --quit "$PLUGIN/tests/load-simdcsv.st"
)
suite() { limit 600 "$WASM_DIR/node/pharo" --headless Pharo.image --no-default-preferences st --quit "$LANE_ST"; }
# (a line of 32 bytes: 3 fields, one of them quoted with a comma in it)
big() (
    PHARO_WASM_OLD_SPACE_BASE=0x40000000; export PHARO_WASM_OLD_SPACE_BASE
    $P eval "| line b n offsets |
	line := ('1234567890,\"abc, def\",ABCDEFGHI' , String lf) asByteArray.
	b := ByteArray new: 32 * 1024 * 1024.
	b replaceFrom: 1 to: line size with: line startingAt: 1.
	n := line size.
	[ n < b size ] whileTrue: [ b replaceFrom: n + 1 to: 2 * n with: b startingAt: 1. n := 2 * n ].
	offsets := SimdCSV separatorOffsetsIn: b.
	{ line size. SimdCSV count: b. offsets size. offsets last }"
)
garbage() {
    $P eval "| line b n rows old |
	line := ('1234,abcdef,789' , String lf) asByteArray.
	b := ByteArray new: 1024 * 1024.
	b replaceFrom: 1 to: line size with: line startingAt: 1.
	n := line size.
	[ n < b size ] whileTrue: [ b replaceFrom: n + 1 to: 2 * n with: b startingAt: 1. n := 2 * n ].
	n := 0.
	1 to: 200 do: [ :i |
		rows := SimdCSV rowsFrom: b.
		n := n + rows size.
		i = 20 ifTrue: [ old := Smalltalk vm parameterAt: 1 ] ].
	{ line size. n. (Smalltalk vm parameterAt: 1) <= old }"
}

# web: simdcsv-lane.st evaluated in the REPL of the Console page, whose
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
      { path: "/pharo/simdcsv-lane.st", data: fs.readFileSync(script) },
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
  drv.feed("Smalltalk compiler evaluate: \x27/pharo/simdcsv-lane.st\x27 asFileReference contents\n");
  await prompt(at);
  process.stdout.write(out.slice(at));
  process.exit(0);
})().catch(e => { process.stdout.write(out.slice(-2000)); console.error("web: " + e.message); process.exit(1); });
' "$SRCDIR" "$WASM_DIR/web" "$loaded" "$LANE_ST"
}

# suite_passed NAME: the line of simdcsv-lane.st in smoke.out, 'SimdCSV-Tests:
# <run> run, <failures> failures, <errors> errors, <skipped> skipped, backend
# #<backend>', has tests, no failure, no error, none skipped, and the backend
# #plugin (without the plugin half of the tests would skip themselves, and
# pass)
suite_passed() {
    summary=$(grep '^SimdCSV-Tests: ' smoke.out | head -n 1)
    set -- "$1" $summary
    if test "$status" -ne 0; then
	fail "$1" "exit status $status"
    elif test -z "$summary"; then
	fail "$1" "no 'SimdCSV-Tests:' line"
    elif ! is_number "$3" || test "$3" -eq 0; then
	fail "$1" "no test ran: '$summary'"
    elif test "x$5" != x0 || test "x$7" != x0 || test "x$9" != x0; then
	fail "$1" "$summary: $(grep -E '^(failed|error|skipped) ' smoke.out | head -n 5 | tr '\n' ' ')"
    elif test "x${12}" != 'x#plugin'; then
	fail "$1" "not on the plugin: '$summary'"
    else
	ok "$1" "$3 tests"
    fi
}
# probed NAME LINE: ci-load.st printed the line LINE and no other
probed() {
    if test "$status" -ne 0; then
	fail "$1" "exit status $status: $(grep '^SimdCSVPlugin' smoke.out | head -n 1)"
    elif test "x$(grep '^SimdCSVPlugin' smoke.out)" != "x$2"; then
	fail "$1" "expected '$2', got '$(grep '^SimdCSVPlugin' smoke.out | head -n 1)'"
    else
	ok "$1"
    fi
}

fresh builtin
expect builtin true $P eval "Smalltalk vm listBuiltinModules includes: 'SimdCSVPlugin (i)'"

fresh probe
if run probe probe; then
    probed probe 'SimdCSVPlugin emscripten fallback a9494a8 1 forced - ok'
fi
fresh forced
if run forced probe_avx2; then
    probed forced 'SimdCSVPlugin emscripten fallback a9494a8 1 forced refused avx2 ok'
fi

fresh load
if run load load; then
    if test "$status" -ne 0; then
	fail load "exit status $status: $(head -n 1 smoke.err)"
    elif ! grep '^SimdCSV: loaded, protocol 1, plugin SimdCSVPlugin 1\.0 simdcsv a9494a8 fallback emscripten' smoke.out >/dev/null; then
	fail load "no 'SimdCSV: loaded, protocol 1, plugin SimdCSVPlugin 1.0 simdcsv a9494a8 fallback emscripten'"
    elif ! rm -rf "$loaded" || ! mkdir -p "$loaded" || ! cp -P Pharo.image Pharo.changes ./*.sources "$loaded/"; then
	fail load "cannot keep the image in $loaded"
    else
	ok load
    fi
fi
if ! test -f "$loaded/Pharo.image"; then
    finish 79-simdcsv
fi

fresh info "$loaded"
expect info "#(#(1 'SimdCSVPlugin 1.0' 'a9494a8' 'fallback' 'emscripten' #('fallback') nil 4294967294) #plugin)" $P eval \
    "{ SimdCSV pluginInfo. SimdCSV backend }"

fresh suite "$loaded"
if run suite suite; then
    suite_passed suite
fi

fresh big "$loaded"
expect big "#(32 $((1024 * 1024)) $((3 * 1024 * 1024)) $((32 * 1024 * 1024 - 1)))" big

fresh garbage "$loaded"
expect garbage "#(16 $((200 * 65536)) true)" garbage

fresh web "$loaded"
if run web web; then
    suite_passed web
fi

rm -rf "$loaded"
echo "79-simdcsv: $(( $(date +%s) - started )) s"
finish 79-simdcsv
