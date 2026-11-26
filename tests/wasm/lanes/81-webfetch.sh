#!/bin/sh
# 81-webfetch - fetch for the image: the WebFetchPlugin and, when it is there,
# the Smalltalk package WebFetch
#
# The checks that need no Smalltalk package (src/emscripten/plugins/
# WebFetchPlugin.c, packaging/emscripten/host, docs/WebFetch.md), against the
# local server of tests/wasm/lib/fetch-http.mjs, on copies of the stock image:
#
#   unit     tests/wasm/web-fetch.test.mjs: host-events.js and web-fetch.js
#            with a fake fetch, node-pump.js with a fake module; and
#            tests/wasm/fetch-http.test.mjs, the server itself;
#   builtin  Smalltalk vm listBuiltinModules has 'WebFetchPlugin (i)';
#   codes    tests/wasm/st/webfetch-lane.st in the node VM: what the six
#            primitives answer, and every failure code on bad arguments; the
#            kinds of error; 64 requests open and one more; 150 times a
#            semaphore that takes the index of one that was just closed: no
#            signal that is not its own;
#   callback start, state, head, read and close inside an FFI callback, where
#            primitiveMayWait is false (skipped without the FFI);
#   memory   60 MB in one object costs one copy in old space, and 150 MB more
#            grow the memory;
#   nomemory a body that JavaScript cannot copy: 'insufficient C memory';
#   unavailable  a node without fetch: availability 0, 'unsupported operation';
#   off      PHARO_WASM_NETWORK=0: availability 2, 'unsupported operation',
#            and, with the smart-HTTP transport of libgit2, a clone that says
#            so and makes no request;
#   missing  the host VM, without the plugin: 'not found' (skipped without
#            HOST_PHARO);
#   exit     three requests open at Smalltalk exit: 3: node ends within 2 s,
#            with that status, and the server sees them close;
#   trap     the same at a trap of the VM: exit status 1;
#   web      the probe codes in pharo-web (the MEMORY64=2 module of the
#            pages), through packaging/emscripten/web/vm-driver.js and the
#            REPL of the Console page.
#
# With WEB_FETCH_DIR, a checkout of the package (web-fetch.st):
#
#   load     tests/load-webfetch.st of the checkout loads the groups Core,
#            Tests, Zinc and ZincTests with Metacello, and the image is saved;
#   suite    tests/run-webfetch.st runs the packages WebFetch-Tests and
#            WebFetch-Zinc-Tests in the node VM, with the live tests: no
#            failure, no error, and no test skipped but the two classes for
#            VMs without the plugin, and WebFetchEchoServerTest, which goes to
#            a public server only when WEB_FETCH_ECHO_SERVER names it;
#   suite off  the same with PHARO_WASM_NETWORK=0: no failure, no error, and
#            the live tests skipped;
#   suite web  the suite in pharo-web;
#   suite callback  tests/callback-webfetch.st: requests of the callback and
#            of other processes while a real FFI callback runs (skipped
#            without the FFI);
#   snapshot  tests/snapshot-webfetch.st, twice: an image saved while processes
#            wait for requests goes on, and restarted it signals
#            WebFetchSessionEnded to each of them, and fetches again.
#
# Without it these are one skipped check.  The load and the suites are
# stopped after 600 s each.
#
# Environment (from make wasm-check): NODE, WASM_DIR, HOST_PHARO, SRCDIR,
# TEST_DIR, WASM_CHECK_TIMEOUT (see lib/common.sh); and WEB_FETCH_DIR.

SRCDIR=$(cd "${SRCDIR:-$(dirname "$0")/../../..}" && pwd)
TEST_DIR=${TEST_DIR:+$TEST_DIR/81-webfetch}
# (the defaults of WASM_DIR, TEST_DIR and NODE, and the checks)
. "$SRCDIR/tests/wasm/lib/common.sh"
lane=webfetch
started=$(date +%s)
LANE_ST=$SRCDIR/tests/wasm/st/webfetch-lane.st
WEB_FETCH_DIR=${WEB_FETCH_DIR-}
if test -n "$WEB_FETCH_DIR"; then
    if ! test -d "$WEB_FETCH_DIR/src/BaselineOfWebFetch"; then
	echo "FAIL 81-webfetch: no $WEB_FETCH_DIR/src/BaselineOfWebFetch (WEB_FETCH_DIR)"
	exit 1
    fi
    WEB_FETCH_DIR=$(cd "$WEB_FETCH_DIR" && pwd)
    export WEB_FETCH_DIR
fi
unset PHARO_WASM_NETWORK WEB_FETCH_SERVER WEB_FETCH_GROUPS WEBFETCH_LANE_GIT

for f in "$WASM_DIR/node/pharo" "$WASM_DIR/node/pharo.js" "$WASM_DIR/web/pharo-web.js" "$WASM_DIR/web/pharo-web.wasm"; do
    if ! test -f "$f"; then
	echo "FAIL 81-webfetch: no $f (make wasm)"
	exit 1
    fi
done
cached() { grep -Eiq "^$1:BOOL=(ON|TRUE|YES|Y|1)\$" "$WASM_DIR/cmake/CMakeCache.txt" 2>/dev/null; }

# The server, with every request and whether the client went before its end
# in <url>/_stats
"$NODE" "$SRCDIR/tests/wasm/lib/fetch-http.mjs" >"$TEST_DIR/server.url" 2>"$TEST_DIR/server.err" &
server=$!
trap 'kill $server 2>/dev/null' EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM
n=0
while ! test -s "$TEST_DIR/server.url" && test $n -lt 100; do n=$((n + 1)); sleep 0.1; done
url=$(cat "$TEST_DIR/server.url")
if test -z "$url"; then
    echo "FAIL 81-webfetch: the server did not start: $(cat "$TEST_DIR/server.err")"
    exit 1
fi
echo "81-webfetch: $WASM_DIR/node/pharo, Pharo $(cat "$WASM_DIR/image/stock/pharo.version" 2>/dev/null), server $url"

# vm ARG ...: the node VM on the image of the check; lane PROBE: the probe;
# st_probe PROBE: webfetch-lane.st in it
vm() { limit "${vm_limit:-$timeout}" "$WASM_DIR/node/pharo" --headless Pharo.image --no-default-preferences "$@"; }
probe() {
    echo "$url $1" >webfetch.lane
    vm st --quit "$LANE_ST"
}
probe_off() (
    PHARO_WASM_NETWORK=0; export PHARO_WASM_NETWORK
    if cached WASM_LIBGIT2_HTTP && cached FEATURE_FFI; then WEBFETCH_LANE_GIT=yes; export WEBFETCH_LANE_GIT; fi
    probe off
)
# with_js PROBE: the probe in a node that runs before.cjs first
with_js() (
    NODE_OPTIONS="--require $(pwd)/before.cjs"; export NODE_OPTIONS
    probe "$1"
)
missing() {
    echo "$url missing" >webfetch.lane
    limit "$timeout" "$HOST_PHARO" --headless Pharo.image --no-default-preferences st --quit "$LANE_ST"
}

# probed NAME PROBE [MIN]: the last line of webfetch-lane.st in smoke.out
# says that the probe PROBE passed all of its checks, at least MIN
probed() {
    summary=$(grep "^webfetch-lane $2: " smoke.out | tail -n 1)
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
    limit "$timeout" "$NODE" "$SRCDIR/tests/wasm/fetch-http.test.mjs" &&
	limit "$timeout" "$NODE" "$SRCDIR/tests/wasm/web-fetch.test.mjs"
}
if run unit unit; then
    if test "$status" -ne 0 || grep -q '^not ok' smoke.out; then
	fail unit "$(grep -v '^ok' smoke.out | head -n 6 | tr '\n' ' ')"
    else
	ok unit "$(grep 'passed, ' smoke.out | tr '\n' ' ')"
    fi
fi

fresh builtin
expect builtin true vm eval "Smalltalk vm listBuiltinModules includes: 'WebFetchPlugin (i)'"

fresh codes
if run codes probe codes; then probed codes codes 80; fi

if cached FEATURE_FFI; then
    fresh callback
    if run callback probe callback; then probed callback callback 4; fi
else
    skip_check callback "no FFI: the build has none (WASM_FFI=OFF)"
fi

fresh memory
if run memory probe memory; then probed memory memory 4; fi

fresh nomemory
cat >before.cjs <<'JS'
// the copy of a body of 77777 bytes fails, as that of one too large would
const slice = Uint8Array.prototype.slice;
Uint8Array.prototype.slice = function (from, to) {
  if (to - from === 77777) throw new Error('no memory');
  return slice.call(this, from, to);
};
JS
if run nomemory with_js nomemory; then probed nomemory nomemory 2; fi

fresh unavailable
echo 'delete globalThis.fetch;' >before.cjs
if run unavailable with_js unavailable; then probed unavailable unavailable 3; fi

# git's request, had it been made, would be in the log of the server; with
# git there are 3 checks
off_checks=2
if cached WASM_LIBGIT2_HTTP && cached FEATURE_FFI; then off_checks=3; fi
fresh off
if run off probe_off; then
    if "$NODE" -e 'fetch(process.argv[1] + "/_stats").then(r => r.json()).then(s => process.exit(s.requests.some(r => /^\/git/.test(r.url)) ? 1 : 0))' "$url"; then
	probed off off $off_checks
    else
	fail off "git made a request"
    fi
fi

if test -n "$HOST_PHARO" && test -x "$HOST_PHARO"; then
    fresh missing
    if run missing missing; then probed missing missing 3; fi
else
    skip_check missing "no HOST_PHARO"
fi

# ended PROBE STATUS: the VM runs the probe, which says 'ending' with three
# requests open, and ends; prints the status and the time from there to the
# end, and the number of requests that the server saw close
ended() {
    echo "$url $1" >webfetch.lane
    limit "$timeout" "$NODE" -e '
const { spawn } = require("child_process");
const [vm, st, url] = process.argv.slice(1);
const closed = async () => (await (await fetch(url + "/_stats")).json()).closed;
(async () => {
  const before = await closed();
  const p = spawn(process.execPath, [vm, "--headless", "Pharo.image", "--no-default-preferences", "st", "--quit", st]);
  let out = "", at = 0, ended = 0;
  p.stdout.on("data", d => { out += d; if (!at && /ending with true\n/.test(out)) at = performance.now(); });
  p.stderr.on("data", d => { require("fs").appendFileSync("vm.err", d); });
  p.on("exit", () => { ended = performance.now(); });
  p.on("close", async code => {
    const ms = at ? ended - at : -1;
    for (let i = 0; i < 20 && await closed() < before + 3; i++) await new Promise(f => setTimeout(f, 100));
    console.log(`status ${code}, ${ms >= 0 && ms < 2000 ? "in time" : ms.toFixed(0) + " ms"}, ${await closed() - before} closed`);
    if (!at || /not ok|not trapped/.test(out)) console.log(out);
  });
})();' "$WASM_DIR/node/pharo.js" "$LANE_ST" "$url"
}
fresh exit
expect exit 'status 3, in time, 3 closed' ended exit
fresh trap
expect trap 'status 1, in time, 3 closed' ended trap

# web_run IMAGE_DIR SCRIPT PROBE: the script in the REPL of the Console page,
# whose output from there to the next prompt it prints
web_run() {
    limit 600 "$NODE" -e '
const fs = require("fs"), path = require("path");
const [srcDir, webDir, imageDir, script, lane] = process.argv.slice(1);
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
      { path: "/pharo/lane.st", data: fs.readFileSync(script) },
      { path: "/pharo/webfetch.lane", data: lane },
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
  drv.feed("Smalltalk compiler evaluate: \x27/pharo/lane.st\x27 asFileReference contents\n");
  await prompt(at);
  process.stdout.write(out.slice(at));
  process.exit(0);
})().catch(e => { process.stdout.write(out.slice(-2000)); console.error("web: " + e.message); process.exit(1); });
' "$SRCDIR" "$WASM_DIR/web" "$1" "$2" "$url $3"
}
fresh web
if run web web_run "$(pwd)" "$LANE_ST" codes; then probed web codes 80; fi

if test -z "$WEB_FETCH_DIR"; then
    skip_check package "no WEB_FETCH_DIR, a checkout of the package WebFetch (web-fetch.st)"
    echo "81-webfetch: $(( $(date +%s) - started )) s"
    finish 81-webfetch
    exit 0
fi

# (the image with the package, which load saves)
loaded=$TEST_DIR/webfetch-image
vm_limit=600
load() { vm st --save --quit "$WEB_FETCH_DIR/tests/load-webfetch.st"; }
suite() {
    echo "$url" >webfetch.lane
    vm st --quit "$WEB_FETCH_DIR/tests/run-webfetch.st"
}
suite_off() (
    PHARO_WASM_NETWORK=0; export PHARO_WASM_NETWORK
    suite
)
callback() {
    echo "$url" >webfetch.lane
    vm st --quit "$WEB_FETCH_DIR/tests/callback-webfetch.st"
}
snapshot() {
    echo "$url" >webfetch.lane
    vm st "$WEB_FETCH_DIR/tests/snapshot-webfetch.st"
}
# suites_passed NAME AVAILABILITY SKIPPED: both lines of run-webfetch.st have
# tests, no failure, no error and the availability, and only the classes that
# match SKIPPED have a test skipped
suites_passed() {
    note=
    for package in WebFetch-Tests WebFetch-Zinc-Tests; do
	summary=$(grep "^$package: " smoke.out | tail -n 1)
	others=$(grep '^skipped ' smoke.out | grep -Ev "$3" | head -n 3 | tr '\n' ' ')
	set -- "$1" "$2" "$3" $summary
	if test "$status" -ne 0; then
	    fail "$1" "exit status $status"; return
	elif test -z "$summary" || ! is_number "$5" || test "$5" -eq 0; then
	    fail "$1" "no test of $package ran: '$summary'"; return
	elif test "x$7" != x0 || test "x$9" != x0; then
	    fail "$1" "$summary: $(grep -E '^(failed|error) ' smoke.out | head -n 5 | tr '\n' ' ')"; return
	elif test "x${13}" != "x$2"; then
	    fail "$1" "not $2: '$summary'"; return
	elif test -n "$others"; then
	    fail "$1" "skipped: $others"; return
	fi
	note="$note${note:+, }$5 tests of $package"
    done
    ok "$1" "$note, $(grep -c '^skipped ' smoke.out) skipped"
}

fresh load
if run load load; then
    if test "$status" -ne 0; then
	fail load "exit status $status: $(head -n 1 smoke.err)"
    elif ! grep '^WebFetch: loaded .*, ready$' smoke.out >/dev/null; then
	fail load "no 'WebFetch: loaded ..., ready': $(grep '^WebFetch:' smoke.out)"
    elif ! rm -rf "$loaded" || ! mkdir -p "$loaded" || ! cp -P Pharo.image Pharo.changes ./*.sources "$loaded/"; then
	fail load "cannot keep the image in $loaded"
    else
	ok load
    fi
fi
if test -f "$loaded/Pharo.image"; then
    fresh suite "$loaded"
    if run suite suite; then suites_passed suite ready 'NativeTest|EchoServerTest'; fi
    fresh suite-off "$loaded"
    if run 'suite off' suite_off; then suites_passed 'suite off' off 'NativeTest|LiveTest|EchoServerTest'; fi
    fresh suite-web "$loaded"
    if run 'suite web' web_run "$(pwd)" "$WEB_FETCH_DIR/tests/run-webfetch.st" suite; then
	suites_passed 'suite web' ready 'NativeTest|EchoServerTest'
    fi
    if cached FEATURE_FFI; then
	fresh suite-callback "$loaded"
	expect 'suite callback' 'callback: 9 of 9' callback
    else
	skip_check 'suite callback' "no FFI: the build has none (WASM_FFI=OFF)"
    fi
    fresh snapshot "$loaded"
    expect 'snapshot, saved' 'saved, the slow request: tick0tick1tick2tick3tick4' snapshot
    expect 'snapshot, restarted' 'restarted: 6 of 6 WebFetchSessionEnded, open 0, then hello' snapshot
    rm -rf "$loaded"
fi
echo "81-webfetch: $(( $(date +%s) - started )) s"
finish 81-webfetch
