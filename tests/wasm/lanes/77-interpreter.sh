#!/bin/sh
# 77-interpreter - the interpreter against its golden files and a native VM
#
# tests/wasm/st/interp-arith.st (the arithmetic and the comparisons),
# interp-objects.st (at: and at:put:, size, new, the write barrier and the
# method cache), interp-control.st (sends, blocks, jumps, contexts,
# exceptions, processes) and interp-methods.st (methods that become others)
# exercise every bytecode, and every primitive, that the fast interpreter
# of WASM_FAST_INTERP=ON has by hand (src/emscripten/fastinterp), on copies
# of the stock image:
#
#   <program>         in build-wasm/node/pharo: what it prints must be the
#                     golden file tests/wasm/golden/interp-<program>.txt,
#                     the same for Pharo 12 to 15, with nothing on stderr
#                     (but the assertions of Emscripten in a Debug build); a
#                     line 'DIFF ...' is a bytecode that does not answer
#                     what the primitive of its selector does;
#   <program>-1ms     the same in slices of 1 ms (arith, control and
#                     methods): the interpreter is left, and entered again,
#                     everywhere;
#   <program>-web     the same in pharo-web, the module of the pages, through
#                     packaging/emscripten/web/vm-driver.js, in node (objects
#                     and control);
#   <program>-native  the same on HOST_PHARO, which must agree: the golden
#                     files say what Pharo answers, not what this VM does
#                     (not methods, which a JIT does not cope with).
#
# Without HOST_PHARO the last ones are skipped.  With FASTINTERP_OFF_DIR, the
# directory of a build with WASM_FAST_INTERP=OFF, its node VM runs the
# programs too (<program>-off).  Every run is stopped after
# WASM_CHECK_TIMEOUT (300 here) seconds.  The lane passes with either
# interpreter; docs/WebAssembly.md (Faster interpreters) has the mistakes
# that it was seen to catch.
#
# Environment (from make wasm-check): NODE, WASM_DIR, HOST_PHARO, SRCDIR,
# TEST_DIR, and WASM_CHECK_TIMEOUT (see lib/common.sh).

SRCDIR=$(cd "${SRCDIR:-$(dirname "$0")/../../..}" && pwd)
TEST_DIR=${TEST_DIR:+$TEST_DIR/77-interpreter}
# (the defaults of WASM_DIR, TEST_DIR and NODE, and the checks)
. "$SRCDIR/tests/wasm/lib/common.sh"
lane=interpreter
ST=$SRCDIR/tests/wasm/st
GOLDEN=$SRCDIR/tests/wasm/golden

for f in "$WASM_DIR/node/pharo" "$WASM_DIR/web/pharo-web.js" "$WASM_DIR/web/pharo-web.wasm"; do
    if ! test -f "$f"; then
	echo "FAIL 77-interpreter: no $f (make wasm)"
	exit 1
    fi
done
timeout=${WASM_CHECK_TIMEOUT:-300}
fast=$(sed -n 's/^WASM_FAST_INTERP:[A-Z]*=//p' "$WASM_DIR/cmake/CMakeCache.txt" 2>/dev/null)
echo "77-interpreter: $WASM_DIR/node/pharo, WASM_FAST_INTERP=${fast:-?}, Pharo $(cat "$WASM_DIR/image/stock/pharo.version" 2>/dev/null)"

node_vm() { limit "$timeout" "$WASM_DIR/node/pharo" --headless Pharo.image --no-default-preferences st --quit "$ST/interp-$1.st"; }
slices() ( PHARO_WASM_SLICE_MS=1; export PHARO_WASM_SLICE_MS; node_vm "$1" )
off_vm() { limit "$timeout" "$FASTINTERP_OFF_DIR/node/pharo" --headless Pharo.image --no-default-preferences st --quit "$ST/interp-$1.st"; }
native_vm() { limit "$timeout" "$HOST_PHARO" --headless Pharo.image --no-default-preferences st --quit "$ST/interp-$1.st"; }
web_vm() {
    limit "$timeout" "$NODE" -e '
const fs = require("fs"), path = require("path");
const [srcDir, webDir, script] = process.argv.slice(1);
const createPharoVM = require(path.join(webDir, "pharo-web.js"));
const Driver = require(path.join(srcDir, "packaging", "emscripten", "web", "vm-driver.js"));
const sources = fs.readdirSync(".").filter(f => f.endsWith(".sources"));
const out = fd => (fd === 2 ? process.stderr : process.stdout);
const end = code => { process.exitCode = code; };
Driver.start(createPharoVM, {
  args: ["--headless", "/pharo/Pharo.image", "--no-default-preferences", "st", "--quit", "/pharo/script.st"],
  files: ["Pharo.image", "Pharo.changes", ...sources].map(f => ({ path: "/pharo/" + f, data: fs.readFileSync(f) }))
    .concat({ path: "/pharo/script.st", data: fs.readFileSync(script) }),
  cwd: "/pharo",
  schedule: f => setImmediate(f),
  later: (f, ms) => setTimeout(f, ms),
  cancel: h => clearTimeout(h),
  canRun: () => true,
  onOutput: (fd, bytes) => out(fd).write(Buffer.from(bytes)),
  onState: () => {}, onHost: () => {}, onDiag: () => {},
  onExit: end,
  onCrash: message => { console.error("crash: " + message); end(1); },
}).then(vm => vm.begin(), e => { console.error("web: " + (e && e.stack || e)); end(1); });
' "$SRCDIR" "$WASM_DIR/web" "$ST/interp-$1.st"
}

# compare NAME PROGRAM CMD ...: CMD must print the golden file of PROGRAM
compare() {
    compare_name=$1 program=$2; shift 2
    fresh "$compare_name"
    if run "$compare_name" "$@"; then
	if test "$status" -ne 0; then
	    fail "$compare_name" "exit status $status"
	elif grep -q '^DIFF ' smoke.out; then
	    fail "$compare_name" "$(grep -c '^DIFF ' smoke.out) bytecodes do not answer what their primitives do: $(grep '^DIFF ' smoke.out | head -n 1)"
	elif ! diff "$GOLDEN/interp-$program.txt" smoke.out >diff.out; then
	    fail "$compare_name" "other lines than golden/interp-$program.txt: $(sed -n '/^[<>]/p' diff.out | head -n 2 | cut -c 1-200 | tr '\n' ' ')"
	elif grep -v -e '^Heap resize call ' -e '^program exited ' smoke.err | grep -q .; then
	    # (what Emscripten's assertions of a Debug build say, but nothing else)
	    fail "$compare_name" "stderr is not empty"
	else
	    ok "$compare_name" "$(wc -l <smoke.out | tr -d ' ') lines"
	fi
    fi
}

for p in arith objects control methods; do
    compare $p $p node_vm $p
done
for p in arith control methods; do
    compare $p-1ms $p slices $p
done
for p in objects control; do
    compare $p-web $p web_vm $p
done
for p in arith objects control; do
    if test -n "$HOST_PHARO" && test -x "$HOST_PHARO"; then
	compare $p-native $p native_vm $p
    else
	skip_check $p-native "HOST_PHARO does not name a native Pharo VM"
    fi
done
if test -n "$FASTINTERP_OFF_DIR"; then
    for p in arith objects control methods; do
	compare $p-off $p off_vm $p
    done
fi
finish 77-interpreter
