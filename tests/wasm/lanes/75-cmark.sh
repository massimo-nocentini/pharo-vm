#!/bin/sh
# 75-cmark - the CMarkPlugin (cmark-gfm), built in, and its Smalltalk packages
#
# build-wasm/node/pharo, then build-wasm/web/pharo-web.js, on copies of the
# stock image (plugins/CMarkPlugin, docs/CMark.md):
#
#   builtin   Smalltalk vm listBuiltinModules has 'CMarkPlugin (i)';
#   symbols   every builtin plugin shares one module, and a clean link proves
#             no two of them define the same name (wasm-ld pulls an archive
#             member only for an undefined symbol, so a second definition can
#             shadow the first in silence): llvm-nm --defined-only
#             --extern-only over every archive and object of the link
#             commands of pharo.wasm and pharo-web.wasm in the build tree
#             finds no strong name defined twice (cmark-gfm's houdini_*,
#             _scan_*, create_*_extension, normalize_map_label... included);
#   probe     plugins/CMarkPlugin/tests/ci-load.st, the probe of the CI jobs,
#             on the stock image: the known answers of primitiveInfo,
#             primitiveParse and primitiveRender, then the limits (a C budget
#             answered by the setjmp/longjmp landing, maxDepth, and the
#             special characters of cmark-gfm restored after each landing),
#             and prints 'CMarkPlugin emscripten 0.29.0.gfm.14 1 ok' and
#             'CMarkPlugin limits ok';
#   load      plugins/CMarkPlugin/tests/load-cmark.st loads the groups Core
#             and Tests of BaselineOfCMark with Metacello from the submodule
#             plugins/CMarkPlugin/cmark-gfm.st, finds the plugin, and the image
#             is saved: the checks below run on copies of it;
#   info      CMarkPrimitives pluginInfo is what primitiveInfo answers on the
#             web: protocol 1, 'emscripten', a default C budget of 128 MiB and
#             a kept result of at most 8 MiB (the WebAssembly limits);
#   suite     tests/wasm/st/cmark-lane.st runs the package CMark-Tests: no
#             failure, no error and no test skipped (the tests of the plugin
#             skip themselves without it; the large inputs scale down on
#             WebAssembly); it prints the free old space before and after,
#             and Smalltalk growMemoryByAtLeast: 64 MiB must still succeed
#             after the suite (the C heap, which never shrinks, did not take
#             the window of old space);
#   roassal   when the stock image has Roassal (RSComposite): the groups
#             Roassal and RoassalTests loaded too, then the package
#             CMark-Roassal-Tests with no failure and no error (the shapes and
#             the metrics of RSLabel through cairo and FreeType of the FFI's
#             registry); its canvas tests skip when RSCanvas new fails and its
#             exports when no cairo surface can be made, and each skip is
#             printed;
#   web       the suite of CMark-Tests again in pharo-web (the MEMORY64=2
#             module of the pages), through packaging/emscripten/web/
#             vm-driver.js and the REPL of the Console page, as lane 73 runs
#             UtilsPlugin's.
#
# Skipped when the CMake cache of the build turns FEATURE_PLUGIN_CMARK off.
# Fails when the submodule is not checked out (git submodule update --init
# plugins/CMarkPlugin/cmark-gfm.st).  CMark is written for Pharo 12 to 15:
# every stock image runs the lane.  The load, the suite, the Roassal check and
# the web run are each stopped after 600 s; the lane prints its time.
#
# Environment (from make wasm-check): NODE, WASM_DIR, SRCDIR, TEST_DIR, and
# WASM_CHECK_TIMEOUT (see lib/common.sh); LLVM_NM, the llvm-nm of the
# symbols check (by default the one next to the emcc of the build).

SRCDIR=$(cd "${SRCDIR:-$(dirname "$0")/../../..}" && pwd)
TEST_DIR=${TEST_DIR:+$TEST_DIR/75-cmark}
# (the defaults of WASM_DIR, TEST_DIR and NODE, and the checks)
. "$SRCDIR/tests/wasm/lib/common.sh"
lane=cmark
started=$(date +%s)
PLUGIN=$SRCDIR/plugins/CMarkPlugin
LANE_ST=$SRCDIR/tests/wasm/st/cmark-lane.st
cache=$WASM_DIR/cmake/CMakeCache.txt

if ! grep -Eiq '^FEATURE_PLUGIN_CMARK:BOOL=(ON|TRUE|YES|Y|1)$' "$cache" 2>/dev/null; then
    skip "the build has no CMarkPlugin (FEATURE_PLUGIN_CMARK is off)"
fi
for f in "$WASM_DIR/node/pharo" "$WASM_DIR/web/pharo-web.js" "$WASM_DIR/web/pharo-web.wasm" \
    "$PLUGIN/tests/ci-load.st" "$PLUGIN/tests/load-cmark.st" "$LANE_ST"; do
    if ! test -f "$f"; then
	echo "FAIL 75-cmark: no $f"
	exit 1
    fi
done
if ! test -d "$PLUGIN/cmark-gfm.st/src/BaselineOfCMark"; then
    echo "FAIL 75-cmark: no $PLUGIN/cmark-gfm.st/src (git submodule update --init plugins/CMarkPlugin/cmark-gfm.st)"
    exit 1
fi
version=$(cat "$WASM_DIR/image/stock/pharo.version" 2>/dev/null)
echo "75-cmark: $WASM_DIR/node/pharo, Pharo $version"

pharo_run() { limit "$timeout" "$WASM_DIR/node/pharo" "$@"; }
P="pharo_run --headless Pharo.image --no-default-preferences"
# (the image with CMark and its tests, which load saves)
loaded=$TEST_DIR/cmark-image

# symbols: the strong names (not W or V) that two inputs of the link command
# in link.txt of the build tree define, as 'name input input...'; the inputs
# are its .a and .o operands, each once (CMake repeats some archives)
duplicates() (
    cd "$WASM_DIR/cmake" || exit 1
    inputs=$(tr ' ' '\n' <"$1" | grep '\.[ao]$' | awk '!seen[$0]++')
    test -n "$inputs" || { echo "no archive or object in $1" >&2; exit 1; }
    # -A: '<archive>:<member>: <address> <type> <name>', or '<object>: ...'
    # shellcheck disable=SC2086
    "$nm" -A --defined-only --extern-only $inputs >nm.out 2>nm.err || { cat nm.err >&2; exit 1; }
    awk '{ type = $(NF - 1); if (type ~ /^[WVwv]$/) next;
	   input = $1; sub(/:$/, "", input); print $NF, input }' nm.out \
	| sort | awk '{ count[$1]++; where[$1] = where[$1] " " $2 }
	END { for (name in count) if (count[name] > 1) print name where[name] }' | sort
    rm -f nm.out nm.err
)
symbols() {
    for target in pharo pharo-web; do
	link=$WASM_DIR/cmake/CMakeFiles/$target.dir/link.txt
	if ! test -f "$link"; then
	    echo "no link command $link" >&2
	    return 1
	fi
	found=$(duplicates "$link") || return 1
	count=$(printf '%s' "$found" | grep -c .)
	echo "$target: $count duplicate names"
	if test -n "$found"; then printf '%s\n' "$found" | head -20; fi
    done
}
nm=${LLVM_NM:-}
if test -z "$nm"; then
    # (the emcc of the build is <emsdk>/upstream/emscripten/emcc, and llvm-nm
    # is in <emsdk>/upstream/bin)
    emcc=$(sed -n 's/^set(CMAKE_C_COMPILER "\(.*\)")$/\1/p' "$WASM_DIR"/cmake/CMakeFiles/*/CMakeCCompiler.cmake 2>/dev/null | head -n 1)
    if test -n "$emcc" && test -x "$(dirname "$(dirname "$emcc")")/bin/llvm-nm"; then
	nm=$(dirname "$(dirname "$emcc")")/bin/llvm-nm
    elif test -n "${EMSDK:-}" && test -x "$EMSDK/upstream/bin/llvm-nm"; then
	nm=$EMSDK/upstream/bin/llvm-nm
    else
	nm=$(command -v llvm-nm 2>/dev/null)
    fi
fi

probe() { $P st "$PLUGIN/tests/ci-load.st"; }
load() (
    CMARK_PLUGIN_DIR=$PLUGIN CMARK_GROUPS=$1 CMARK_EXPECT_PLUGIN=yes
    export CMARK_PLUGIN_DIR CMARK_GROUPS CMARK_EXPECT_PLUGIN
    limit 600 "$WASM_DIR/node/pharo" --headless Pharo.image --no-default-preferences \
	st --save --quit "$PLUGIN/tests/load-cmark.st"
)
lane_st() (
    CMARK_LANE_PACKAGE=$1; export CMARK_LANE_PACKAGE
    limit 600 "$WASM_DIR/node/pharo" --headless Pharo.image --no-default-preferences st --quit "$LANE_ST"
)
suite() { lane_st CMark-Tests; }
# roassal: 'CMark: no Roassal' when the image has no RSComposite; else the
# Roassal groups loaded over the image of load, then their tests
roassal() {
    has=$($P eval "Smalltalk globals includesKey: #RSComposite" 2>&1)
    case $has in
	false) echo "CMark: no Roassal"; return 0 ;;
	true) ;;
	*) echo "Smalltalk globals includesKey: #RSComposite answered '$has'"; return 1 ;;
    esac
    load 'Roassal RoassalTests' >roassal-load.out 2>&1 || {
	status=$?
	cat roassal-load.out
	echo "loading Roassal RoassalTests: exit status $status"
	return "$status"
    }
    lane_st CMark-Roassal-Tests
}

# web: cmark-lane.st evaluated in the REPL of the Console page, whose
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
      { path: "/pharo/cmark-lane.st", data: fs.readFileSync(script) },
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
  drv.feed("Smalltalk compiler evaluate: \x27/pharo/cmark-lane.st\x27 asFileReference contents\n");
  await prompt(at);
  process.stdout.write(out.slice(at));
  process.exit(0);
})().catch(e => { process.stdout.write(out.slice(-2000)); console.error("web: " + e.message); process.exit(1); });
' "$SRCDIR" "$WASM_DIR/web" "$loaded" "$LANE_ST"
}

# suite_passed NAME PACKAGE SKIPS: the line of cmark-lane.st in smoke.out,
# '<package>: <run> run, <failures> failures, <errors> errors, <skipped>
# skipped, plugin <platform>', has tests, no failure, no error, the plugin
# 'emscripten', and none skipped unless SKIPS is 'skips' (without the plugin
# the tests would skip themselves, and pass); and the line
# 'CMark: grow 64 MiB ok' follows it
suite_passed() {
    summary=$(grep "^$2: " smoke.out | head -n 1)
    set -- "$1" "$2" "$3" $summary
    if test "$status" -ne 0; then
	fail "$1" "exit status $status"
    elif test -z "$summary"; then
	fail "$1" "no '$2:' line"
    elif ! is_number "$5" || test "$5" -eq 0; then
	fail "$1" "no test ran: '$summary'"
    elif test "x$7" != x0 || test "x$9" != x0; then
	fail "$1" "$summary"
    elif test "x$3" != xskips && test "x${11}" != x0; then
	fail "$1" "$summary"
    elif test "x${14}" != "x'emscripten'"; then
	fail "$1" "the tests did not find the plugin: $summary"
    elif ! grep '^CMark: grow 64 MiB ok' smoke.out >/dev/null; then
	fail "$1" "$(grep '^CMark: grow' smoke.out | head -n 1)"
    else
	before=$(sed -n 's/^CMark: free old space before \([0-9]*\).*/\1/p' smoke.out | head -n 1)
	after=$(sed -n 's/^CMark: free old space after \([0-9]*\).*/\1/p' smoke.out | head -n 1)
	note="$5 tests"
	if test "x${11}" != x0; then note="$note, ${11} skipped: $(grep '^skipped ' smoke.out | sed 's/^skipped //' | tr '\n' ' ')"; fi
	ok "$1" "$note; free old space $before then $after bytes, and 64 MiB more"
    fi
}

fresh builtin
expect builtin true $P eval "Smalltalk vm listBuiltinModules includes: 'CMarkPlugin (i)'"

fresh symbols
if test -z "$nm" || ! test -x "$nm"; then
    fail symbols "no llvm-nm (set LLVM_NM)"
elif run symbols symbols; then
    if test "$status" -ne 0; then
	fail symbols "exit status $status: $(head -n 1 smoke.err)"
    elif test "$(grep -c '^pharo[a-z-]*: 0 duplicate names$' smoke.out)" -ne 2; then
	fail symbols "$(grep -v ': 0 duplicate names$' smoke.out | head -n 5 | tr '\n' ' ')"
    else
	ok symbols "pharo.wasm and pharo-web.wasm"
    fi
fi

fresh probe
if run probe probe; then
    if test "$status" -ne 0; then
	fail probe "exit status $status: $(grep '^CMarkPlugin' smoke.out | head -n 1)"
    elif ! grep '^CMarkPlugin emscripten 0\.29\.0\.gfm\.14 1 ok' smoke.out >/dev/null; then
	fail probe "no 'CMarkPlugin emscripten 0.29.0.gfm.14 1 ok'"
    elif ! grep '^CMarkPlugin limits ok' smoke.out >/dev/null; then
	fail probe "no 'CMarkPlugin limits ok'"
    else
	ok probe
    fi
fi

fresh load
if run load load 'Core Tests'; then
    if test "$status" -ne 0; then
	fail load "exit status $status: $(head -n 1 smoke.err)"
    elif ! grep '^CMark: loaded, protocol 1, plugin CMarkPlugin 1\.0 cmark-gfm 0\.29\.0\.gfm\.14 emscripten' smoke.out >/dev/null; then
	fail load "no 'CMark: loaded, protocol 1, plugin CMarkPlugin 1.0 cmark-gfm 0.29.0.gfm.14 emscripten'"
    elif ! rm -rf "$loaded" || ! mkdir -p "$loaded" || ! cp -P Pharo.image Pharo.changes ./*.sources "$loaded/"; then
	fail load "cannot keep the image in $loaded"
    else
	ok load
    fi
fi
if ! test -f "$loaded/Pharo.image"; then
    finish 75-cmark
fi

fresh info "$loaded"
expect info "#(1 'emscripten' 134217728 8388608)" $P eval \
    "| i | i := CMarkPrimitives pluginInfo. { i first. i at: 4. (i at: 6) at: 3. (i at: 6) at: 5 }"

fresh suite "$loaded"
if run suite suite; then
    suite_passed suite CMark-Tests noskips
fi

fresh roassal "$loaded"
if run roassal roassal; then
    if test "$status" -eq 0 && grep '^CMark: no Roassal' smoke.out >/dev/null; then
	skip_check roassal "the stock image has no Roassal (RSComposite)"
    else
	suite_passed roassal CMark-Roassal-Tests skips
    fi
fi

fresh web "$loaded"
if run web web; then
    suite_passed web CMark-Tests noskips
fi

rm -rf "$loaded"
echo "75-cmark: $(( $(date +%s) - started )) s"
finish 75-cmark
