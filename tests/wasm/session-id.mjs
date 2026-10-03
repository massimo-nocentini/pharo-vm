// session-id.mjs - a restored snapshot gets a VM session ID of its own
//
// usage: node session-id.mjs WEB_DIR IMAGE_DIR
//
// The FilePlugin keeps the VM session ID in every file handle it answers,
// and trusts a handle only while that ID is the current one: this is how
// the handles that a snapshot brings back from the session that saved it
// are told from live ones.  So a session must never get the ID of the
// session that saved its image.  It could:
// StackInterpreter>>initializeGlobalSessionID added the Unix time in
// seconds to ioMSecs, which counts the milliseconds since the VM started,
// so two sessions D seconds apart got the same ID whenever the later one
// set it D milliseconds sooner after its start.  The restored image then
// closed the saved session's FILE*s of its source files, and trapped.
//
// This boots the web VM (WEB_DIR/pharo-web.js through vm-driver.js, as
// the Console page does) twice, under a clock of its own that stands still
// during the first slice, the one in which the VM reads the image and sets
// its session ID; IMAGE_DIR holds the stock image (build-wasm/image/stock):
//  - A boots the stock image at T, keeps a file handle in a global, saves
//    itself, and quits right after writing a line it does not flush (which
//    must still arrive: the VM flushes the C streams when it exits);
//  - B boots that snapshot at T + 500 ms, in the same second, where the
//    old formula gives B exactly A's ID.
// B must boot, have an ID other than A's, refuse A's file handle and read
// method sources, then end with status 0 at the end of its input.  What the
// emscripten runtime and the driver say (onDiag: print, printErr and the
// callbacks that threw) is copied to stderr; an engine-level failure in it,
// in what a VM printed or in a crash, or a callback that threw, fails a
// check too.  Prints a line per check, and exits 1 if any fails.  Lane 25
// (tests/wasm/lanes/25-session-id.sh) runs it.

import fs from 'node:fs';
import path from 'node:path';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const srcDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
const webDir = path.resolve(process.argv[2] || path.join(srcDir, 'build-wasm', 'web'));
const imageDir = path.resolve(process.argv[3] || path.join(webDir, '..', 'image', 'stock'));
const createPharoVM = require(path.join(webDir, 'pharo-web.js'));
const Driver = require(path.join(srcDir, 'packaging', 'emscripten', 'web', 'vm-driver.js'));
const wasmModule = new WebAssembly.Module(fs.readFileSync(path.join(webDir, 'pharo-web.wasm')));
const replSource = fs.readFileSync(path.join(srcDir, 'packaging', 'emscripten', 'st', 'web-repl.st'));

const only = ext => {
  const names = fs.readdirSync(imageDir).filter(f => f.endsWith(ext));
  if (names.length !== 1) throw new Error(`expected one ${ext} in ${imageDir}, found ${names.length}`);
  return names[0];
};
const imageName = only('.image');
const sourcesName = only('.sources');
const files = (image, changes) => [
  { path: '/pharo/Pharo.image', data: image },
  { path: '/pharo/Pharo.changes', data: changes },
  { path: '/pharo/' + sourcesName, data: fs.readFileSync(path.join(imageDir, sourcesName)) },
  { path: '/pharo/st/web-repl.st', data: replSource },
];

// The clock of the VM: emscripten reads CLOCK_REALTIME, and so time() and
// gettimeofday(), from Date.now().  It stands at `base' until the first
// slice is over, then runs from there.
const realNow = Date.now;
let clock = null;
Date.now = () => !clock ? realNow.call(Date)
  : clock.from === null ? clock.base : clock.base + Math.floor(performance.now() - clock.from);

// The session ID of a VM whose clock read `ms' (Unix milliseconds) when it
// set it, and ioMSecs 0, by the old formula and by the new one (the UTC
// clock of the VM counts from 1901).
const MASK = 2 ** 31;
const oldId = ms => Math.floor(ms / 1000) % MASK;
const newId = ms => (Math.floor(ms / 1000) + ms + 2177452800000) % MASK;

// What no session may say: an engine-level failure (as vm-harness.js), or,
// in onDiag, that a callback of this host threw
const ENGINE_ERRORS = /RuntimeError|RangeError|Aborted|unreachable|signature_mismatch|unsupported syscall/;
const CALLBACK_THREW = /^vm-driver: [^\n]*/m;
const sessions = [];

let failures = 0, checks = 0;
function check(ok, what, detail) {
  checks += 1;
  if (ok) console.log('ok ' + checks + ' ' + what);
  else { failures += 1; console.log('not ok ' + checks + ' ' + what + (detail ? ': ' + detail : '')); }
}
function finish() {
  for (const s of sessions) {
    const found = (s.out + s.diag + (s.crash || '')).match(ENGINE_ERRORS) || s.diag.match(CALLBACK_THREW);
    if (found) check(false, `no engine error or callback that threw in session ${s.name}`, found[0]);
  }
  Date.now = realNow;
  console.log(failures ? `session-id: ${failures} of ${checks} checks FAILED` : `session-id: all ${checks} checks passed`);
  process.exit(failures ? 1 : 0);
}

let waiters = [];
const notify = () => { const w = waiters; waiters = []; for (const f of w) f(); };
async function waitFor(what, pred, ms) {
  const t0 = performance.now();
  for (;;) {
    const v = pred();
    if (v) return v;
    if (performance.now() - t0 > ms) throw new Error('timed out after ' + ms + ' ms waiting for ' + what);
    await new Promise(r => { const t = setTimeout(r, 250); waiters.push(() => { clearTimeout(t); r(); }); });
  }
}

// One VM, `name', booted at `base' with a stopped clock, as the Console
// page boots it
async function boot(name, image, changes, base) {
  const s = { name, out: '', diag: '', at: 0, host: [], exit: null, crash: null };
  sessions.push(s);
  const dec = { 1: new TextDecoder(), 2: new TextDecoder() };
  const own = clock = { base, from: null };
  s.drv = await Driver.start(createPharoVM, {
    args: Driver.vmArgs(),
    files: files(image, changes),
    wasmModule,
    schedule: f => setImmediate(f),
    onOutput: (fd, bytes) => { s.out += dec[fd].decode(bytes, { stream: true }); notify(); },
    onState: () => { if (own.from === null) own.from = performance.now(); notify(); },
    onHost: (kind, text) => { s.host.push(text); notify(); },
    onExit: code => { s.exit = code; notify(); },
    onCrash: (message, stack, stacks) => {
      s.crash = message + (stacks ? '\n' + stacks.split('\n').slice(0, 12).join('\n') : ''); notify();
    },
    onDiag: t => { s.diag += t; process.stderr.write(`[diag ${name}] ` + t + (/\n$/.test(t) ? '' : '\n')); },
  });
  if (!s.drv) throw new Error('the VM did not start: ' + s.crash);
  s.drv.begin();
  // evaluate a line, answer what it printed before the next prompt
  s.eval = async (line, ms = 60000) => {
    s.at = s.out.length;
    s.drv.feed(line + '\n');
    await waitFor(JSON.stringify(line), () => {
      if (s.crash) throw new Error('crash: ' + s.crash);
      if (s.exit !== null) throw new Error('exit ' + s.exit);
      return /st> $/.test(s.out.slice(s.at));
    }, ms);
    return s.out.slice(s.at).replace(/st> $/, '').trim();
  };
  s.prompt = ms => waitFor('the first prompt', () => s.crash || s.exit !== null || /st> $/.test(s.out), ms);
  return s;
}

// The session ID, from a file handle (an SQFile, whose first field it is)
const ID = "| h id | h := File open: '/pharo/Pharo.changes' writable: false. " +
  'id := h unsignedLongAt: 1 bigEndian: false. File close: h. id';

const stockImage = fs.readFileSync(path.join(imageDir, imageName));
const stockChanges = fs.readFileSync(path.join(imageDir, imageName.replace(/\.image$/, '.changes')));

// T: 250 ms into a second, where no formula gives the ID 0 (the VM would
// try again until its clock moved, which this one does not)
let T = Math.floor(realNow.call(Date) / 1000) * 1000 + 250;
while ([oldId(T), oldId(T + 500), newId(T), newId(T + 500)].includes(0)) T += 1000;

try {
  const A = await boot('A', stockImage, stockChanges, T);
  await A.prompt(60000);
  check(!A.crash && A.exit === null, 'A boots the stock image', A.crash || 'exit ' + A.exit);
  const idA = +(await A.eval(ID));
  const formula = idA === newId(T) ? 'the UTC clock' : idA === oldId(T) ? 'seconds + ioMSecs (old)' : 'unknown';
  console.log(`# A: session ID ${idA} (${formula}); old formula ${oldId(T)}, new ${newId(T)}`);
  check(idA > 0 && idA < MASK, 'A has a positive 31-bit session ID', String(idA));
  await A.eval("Smalltalk at: #SessionIdProbe put: (File open: '/pharo/Pharo.changes' writable: false). #kept");
  const saved = await A.eval('Smalltalk snapshot: true andQuit: false', 120000);
  check(/SnapshotOperation/.test(saved) && A.host.length > 0, 'A saves itself', saved);
  const image = A.drv.FS.readFile('/pharo/Pharo.image');
  const changes = A.drv.FS.readFile('/pharo/Pharo.changes');
  // A quits right after writing a line it does not flush: the VM flushes
  // the C streams as it exits (emscriptenExit), so the line still arrives
  A.at = A.out.length;
  A.drv.feed("Stdio stdout nextPutAll: 'unflushed at exit'. Smalltalk exit: 3\n");
  await waitFor('the end of A', () => A.exit !== null || A.crash, 60000);
  check(A.exit === 3 && A.out.slice(A.at).endsWith('unflushed at exit'),
        'A quits with status 3, and its last unflushed line is shown',
        `exit ${A.exit}, crash ${A.crash}, output ${JSON.stringify(A.out.slice(A.at))}`);

  const B = await boot('B', image, changes, T + 500);
  await B.prompt(60000);
  check(!B.crash && B.exit === null, 'B boots the snapshot in the same second',
        B.crash ? 'crashed: ' + B.crash : 'exit ' + B.exit);
  if (B.crash || B.exit !== null) finish();
  const idB = +(await B.eval(ID));
  console.log(`# B: session ID ${idB}; old formula ${oldId(T + 500)}, new ${newId(T + 500)}`);
  check(idB > 0 && idB < MASK && idB !== idA, 'B has a session ID of its own', `A ${idA}, B ${idB}`);
  const probe = await B.eval('File sizeOrNil: (Smalltalk at: #SessionIdProbe)');
  check(probe === 'nil', "B refuses the file handle of A's session", probe);
  const source = await B.eval('(Object >> #yourself) sourceCode');
  check(/yourself/.test(source) && /\^ ?self/.test(source), 'B reads method sources', source);
  // and ends, so that what B says while it shuts down is seen too (a
  // failing check only when it does not end well)
  B.drv.eof();
  await waitFor('the end of B', () => B.exit !== null || B.crash, 60000);
  if (B.crash || B.exit !== 0)
    check(false, 'B ends with status 0 at the end of its input', `exit ${B.exit}, crash ${B.crash}`);
} catch (e) {
  check(false, 'the sessions ran', (e && e.stack) || String(e));
}
finish();
