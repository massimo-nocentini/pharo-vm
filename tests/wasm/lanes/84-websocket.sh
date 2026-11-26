#!/bin/sh
# 84-websocket - WebSockets for the image: the WebSocketPlugin and the
# service of the host
#
# The checks that need no Smalltalk package (src/emscripten/plugins/
# WebSocketPlugin.c, packaging/emscripten/host/web-socket.js), against the
# WebSocket endpoints of the local server of tests/wasm/lib/fetch-http.mjs
# (lib/ws-server.mjs), on copies of the stock image:
#
#   unit     tests/wasm/ws-server.test.mjs, the server, and
#            tests/wasm/web-socket.test.mjs: web-socket.js on a fake
#            WebSocket and fake timers, and the exit guard of node-pump.js;
#   builtin  Smalltalk vm listBuiltinModules has 'WebSocketPlugin (i)';
#   codes    tests/wasm/st/websocket-lane.st in the node VM: what the ten
#            primitives answer, and every failure code on bad arguments; 32
#            sockets and one more; the signals of the three semaphores; the
#            endpoints (echo, subprotocols, deflate, fragments, push,
#            broadcast, the closes of the server, an abrupt end); what fails
#            a socket (a refusal, nobody there, a handshake timeout, a bad
#            URL, tooLarge, overflow, Drop), back-pressure against
#            /ws/slow-read; Send and Close on bad arguments, the codes that
#            the server got, a close that a deaf server never answers, and
#            150 times semaphores that take the indexes of a socket that was
#            just released: no signal that is not their own;
#   lingering  32 sockets to /ws/deaf closed and released: the 33rd open
#            fails, since they linger;
#   callback open, state, receive, send, close and release inside an FFI
#            callback, where primitiveMayWait is false (skipped without the
#            FFI);
#   memory   a message of 60 MB costs one copy in old space;
#   unavailable  a node without WebSocket: availability 0, 'unsupported
#            operation', and fetch still there;
#   off      PHARO_WASM_NETWORK=0: availability 2, 'unsupported operation',
#            and the server saw no connection;
#   missing  the host VM, without the plugin: 'not found' (skipped without
#            HOST_PHARO);
#   exit     three sockets open at Smalltalk exit: 3 (to /ws/echo, a /ws/push
#            that goes on, and /ws/deaf, which never answers the close): node
#            ends within 3 s with that status and its last line of output,
#            its exit guard armed, and the server sees the three end;
#   trap     the same at a trap of the VM: exit status 1;
#   quiet exit  a run without a socket arms no exit guard (exitGuard=0 of
#            PHARO_WASM_STATS);
#   web      the probe codes in pharo-web (the MEMORY64=2 module of the
#            pages), through packaging/emscripten/web/vm-driver.js and the
#            REPL of the Console page;
#   web off  in pharo-web, pharoHost.setNetwork(false) from the harness
#            while a socket is open: it fails with off, the server got 1000
#            'network access is off', the message queued is still read, a new
#            open fails; on again, a socket opens.
#
# Environment (from make wasm-check): NODE, WASM_DIR, HOST_PHARO, SRCDIR,
# TEST_DIR, WASM_CHECK_TIMEOUT (see lib/common.sh).

SRCDIR=$(cd "${SRCDIR:-$(dirname "$0")/../../..}" && pwd)
TEST_DIR=${TEST_DIR:+$TEST_DIR/84-websocket}
# (the defaults of WASM_DIR, TEST_DIR and NODE, and the checks)
. "$SRCDIR/tests/wasm/lib/common.sh"
lane=websocket
started=$(date +%s)
LANE_ST=$SRCDIR/tests/wasm/st/websocket-lane.st
unset PHARO_WASM_NETWORK PHARO_WASM_STATS

for f in "$WASM_DIR/node/pharo" "$WASM_DIR/node/pharo.js" "$WASM_DIR/web/pharo-web.js" "$WASM_DIR/web/pharo-web.wasm"; do
    if ! test -f "$f"; then
	echo "FAIL 84-websocket: no $f (make wasm)"
	exit 1
    fi
done
cached() { grep -Eiq "^$1:BOOL=(ON|TRUE|YES|Y|1)\$" "$WASM_DIR/cmake/CMakeCache.txt" 2>/dev/null; }

# The server, with every WebSocket connection and how it ended in
# <url>/_stats; after its URL and a blank it prints the port of its listener
# that never answers (/ws/hang), the two first fields of websocket.lane
"$NODE" "$SRCDIR/tests/wasm/lib/fetch-http.mjs" --hang-port >"$TEST_DIR/server.url" 2>"$TEST_DIR/server.err" &
server=$!
trap 'kill $server 2>/dev/null' EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM
n=0
while ! test -s "$TEST_DIR/server.url" && test $n -lt 100; do n=$((n + 1)); sleep 0.1; done
served=$(cat "$TEST_DIR/server.url")
url=${served%% *}
if test -z "$url"; then
    echo "FAIL 84-websocket: the server did not start: $(cat "$TEST_DIR/server.err")"
    exit 1
fi
echo "84-websocket: $WASM_DIR/node/pharo, Pharo $(cat "$WASM_DIR/image/stock/pharo.version" 2>/dev/null), server $served"

# vm ARG ...: the node VM on the image of the check; probe PROBE:
# websocket-lane.st in it
vm() { limit "$timeout" "$WASM_DIR/node/pharo" --headless Pharo.image --no-default-preferences "$@"; }
probe() {
    echo "$served $1" >websocket.lane
    vm st --quit "$LANE_ST"
}
probe_off() (
    PHARO_WASM_NETWORK=0; export PHARO_WASM_NETWORK
    probe off
)
# with_js PROBE: the probe in a node that runs before.cjs first
with_js() (
    NODE_OPTIONS="--require $(pwd)/before.cjs"; export NODE_OPTIONS
    probe "$1"
)
missing() {
    echo "$served missing" >websocket.lane
    limit "$timeout" "$HOST_PHARO" --headless Pharo.image --no-default-preferences st --quit "$LANE_ST"
}
# connections NAME TAG: the number of connections that the server logged
# with TAG in their URL
connections() {
    "$NODE" -e 'fetch(process.argv[1] + "/_stats?tag=" + process.argv[2]).then(r => r.json()).then(s => console.log(s.ws.length))' "$url" "$1"
}

# probed NAME PROBE [MIN]: the last line of websocket-lane.st in smoke.out
# says that the probe PROBE passed all of its checks, at least MIN
probed() {
    summary=$(grep "^websocket-lane $2: " smoke.out | tail -n 1)
    set -- "$1" "${3:-1}" $summary
    if test "$status" -ne 0; then
	fail "$1" "exit status $status: $(grep -v '^ok' smoke.out | head -n 3 | tr '\n' ' ')"
    elif test -z "$summary"; then
	fail "$1" "no summary: $(tail -n 2 smoke.out | tr '\n' ' ')"
    elif test "x$7" != x0 || grep -q '^not ok' smoke.out; then
	fail "$1" "$(grep '^not ok' smoke.out | head -n 5 | tr '\n' ' ')"
    elif test "$5" -lt "$2"; then
	fail "$1" "$5 checks, expected $2"
    else
	ok "$1" "$5 checks"
    fi
}

# unit
fresh unit
rm -f Pharo.image Pharo.changes
unit() {
    limit "$timeout" "$NODE" "$SRCDIR/tests/wasm/ws-server.test.mjs" &&
	limit "$timeout" "$NODE" "$SRCDIR/tests/wasm/web-socket.test.mjs"
}
if run unit unit; then
    if test "$status" -ne 0 || grep -q '^not ok' smoke.out; then
	fail unit "$(grep -v '^ok' smoke.out | head -n 6 | tr '\n' ' ')"
    else
	ok unit "$(grep 'passed, ' smoke.out | tr '\n' ' ')"
    fi
fi

fresh builtin
expect builtin true vm eval "Smalltalk vm listBuiltinModules includes: 'WebSocketPlugin (i)'"

fresh codes
if run codes probe codes; then probed codes codes 130; fi

fresh lingering
if run lingering probe lingering; then probed lingering lingering 3; fi

if cached FEATURE_FFI; then
    fresh callback
    if run callback probe callback; then probed callback callback 4; fi
else
    skip_check callback "no FFI: the build has none (WASM_FFI=OFF)"
fi

fresh memory
if run memory probe memory; then probed memory memory 3; fi

fresh unavailable
echo 'delete globalThis.WebSocket;' >before.cjs
if run unavailable with_js unavailable; then probed unavailable unavailable 3; fi

fresh off
if run off probe_off; then
    seen=$(connections off-probe)
    if test "x$seen" != x0; then
	fail off "the server saw $seen connection(s)"
    else
	probed off off 2
    fi
fi

if test -n "$HOST_PHARO" && test -x "$HOST_PHARO"; then
    fresh missing
    if run missing missing; then probed missing missing 3; fi
else
    skip_check missing "no HOST_PHARO"
fi

# ended PROBE: the VM runs the probe, which says 'ending with true' with
# three sockets open, and ends; prints the status, whether it ended within
# 3 s of that line, the last line of its output, its exit guard and the
# number of those connections that the server saw end
ended() {
    echo "$served $1" >websocket.lane
    limit "$timeout" "$NODE" -e '
const { spawn } = require("child_process");
const [vm, st, url] = process.argv.slice(1);
const ended = async () => (await (await fetch(url + "/_stats?tag=t=end")).json()).ws.filter(e => e.ended).length;
(async () => {
  await fetch(url + "/_reset", { method: "POST" });
  const p = spawn(process.execPath, [vm, "--headless", "Pharo.image", "--no-default-preferences", "st", "--quit", st],
    { env: Object.assign({}, process.env, { PHARO_WASM_STATS: "1" }) });
  let out = "", err = "", at = 0, gone = 0;
  p.stdout.on("data", d => { out += d; if (!at && /ending with true\n/.test(out)) at = performance.now(); });
  p.stderr.on("data", d => { err += d; require("fs").appendFileSync("vm.err", d); });
  p.on("exit", () => { gone = performance.now(); });
  p.on("close", async code => {
    const ms = at ? gone - at : -1;
    for (let i = 0; i < 40 && await ended() < 3; i++) await new Promise(f => setTimeout(f, 100));
    const lines = out.trim().split("\n"), guard = (err.match(/exitGuard=(\d)/) || [])[1];
    console.log(`status ${code}, ${ms >= 0 && ms < 3000 ? "in time" : ms.toFixed(0) + " ms"}, last line ${JSON.stringify(lines[lines.length - 1])}, exitGuard=${guard}, ${await ended()} ended`);
    if (!at || /not ok|not trapped/.test(out)) console.log(out);
  });
})();' "$WASM_DIR/node/pharo.js" "$LANE_ST" "$url"
}
fresh exit
expect exit 'status 3, in time, last line "ending with true", exitGuard=1, 3 ended' ended exit
fresh trap
expect trap 'status 1, in time, last line "ending with true", exitGuard=1, 3 ended' ended trap

fresh quiet-exit
quiet() (
    PHARO_WASM_STATS=1; export PHARO_WASM_STATS
    vm eval '3 + 4'
)
if run 'quiet exit' quiet; then
    if test "$status" -ne 0 || test "x$(cat smoke.out)" != x7; then
	fail 'quiet exit' "exit status $status, output '$(cat smoke.out)'"
    elif ! grep -q '^\[pharo-wasm\] exitGuard=0$' smoke.err; then
	fail 'quiet exit' "no 'exitGuard=0' on stderr: $(grep exitGuard smoke.err)"
    else
	ok 'quiet exit' 'exitGuard=0'
    fi
fi

# web_run IMAGE_DIR SCRIPT LANE: the script in the REPL of the Console page,
# whose output from there to the next prompt it prints; the lines
# 'harness: network off' and 'harness: network on' of the script turn the
# network of the VM off and on, as the setting of the pages does
web_run() {
    limit 600 "$NODE" -e '
const fs = require("fs"), path = require("path");
const [srcDir, webDir, imageDir, script, lane] = process.argv.slice(1);
const createPharoVM = require(path.join(webDir, "pharo-web.js"));
const Driver = require(path.join(srcDir, "packaging", "emscripten", "web", "vm-driver.js"));
const only = ext => fs.readdirSync(imageDir).filter(f => f.endsWith(ext))[0];
const image = only(".image"), sources = only(".sources");
const dec = { 1: new TextDecoder(), 2: new TextDecoder() };
let out = "", exit = null, crash = null, waiters = [], drv = null, said = 0;
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
// (out of the slice that printed it)
const harness = () => {
  let at;
  while ((at = out.indexOf("harness: network ", said)) >= 0 && out.indexOf("\n", at) > 0) {
    const on = out.startsWith("harness: network on", at);
    said = out.indexOf("\n", at) + 1;
    setTimeout(() => drv.module.pharoHost.setNetwork(on), 0);
  }
};
(async () => {
  drv = await Driver.start(createPharoVM, {
    args: Driver.vmArgs("console"),
    files: [
      { path: "/pharo/Pharo.image", data: fs.readFileSync(path.join(imageDir, image)) },
      { path: "/pharo/Pharo.changes", data: fs.readFileSync(path.join(imageDir, image.replace(/\.image$/, ".changes"))) },
      { path: "/pharo/" + sources, data: fs.readFileSync(path.join(imageDir, sources)) },
      { path: "/pharo/st/web-repl.st", data: fs.readFileSync(path.join(srcDir, "packaging", "emscripten", "st", "web-repl.st")) },
      { path: "/pharo/lane.st", data: fs.readFileSync(script) },
      { path: "/pharo/websocket.lane", data: lane },
    ],
    schedule: f => setImmediate(f),
    later: (f, ms) => setTimeout(f, ms),
    cancel: h => clearTimeout(h),
    canRun: () => true,
    onOutput: (fd, bytes) => {
      const text = dec[fd].decode(bytes, { stream: true });
      out += text;
      if (fd === 2) process.stderr.write(text);
      harness();
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
  said = at;
  drv.feed("Smalltalk compiler evaluate: \x27/pharo/lane.st\x27 asFileReference contents\n");
  await prompt(at);
  process.stdout.write(out.slice(at));
  process.exit(0);
})().catch(e => { process.stdout.write(out.slice(-2000)); console.error("web: " + e.message); process.exit(1); });
' "$SRCDIR" "$WASM_DIR/web" "$1" "$2" "$served $3"
}
fresh web
if run web web_run "$(pwd)" "$LANE_ST" codes; then probed web codes 130; fi

fresh web-off
if run 'web off' web_run "$(pwd)" "$LANE_ST" weboff; then probed 'web off' weboff 7; fi

echo "84-websocket: $(( $(date +%s) - started )) s"
finish 84-websocket
