// notebook-harness.js - headless tests of the notebook kernel (web-notebook.st) and its client
//
// usage: node [v8 flags] notebook-harness.js WEB_DIR IMAGE_DIR
//
// WEB_DIR is a built web directory (build-wasm/web), IMAGE_DIR an image with
// its .changes and .sources (build-wasm/image/stock, or a Pharo 15 one).
//
// Part K drives the kernel itself through vm-driver.js, as vm-harness.js
// drives the REPL: a copy of the image in MEMFS, with
// packaging/emscripten/st/web-notebook.st of this tree, booted with the
// arguments of vmArgs('notebook') and the events device (events: true).  It
// feeds the requests on stdin, one JSON line each, and reads the events of
// fd 3 through nb-kernel.js's FrameReader: a frame that it rejects fails
// the case.  Part F does the same for what needs a feature of the build
// (manifest.json: the FFI, cairo, FreeType), with a skip line when it is
// absent.  Part C drives the page's client, nb-kernel.js of this tree,
// against the real WEB_DIR/vm-worker.js in worker_threads, behind the shim
// of tests/wasm/lib/worker-shim.js, whose store is the IndexedDB of the
// page: with an image of another Pharo than the site's (another .sources),
// that image is the Console's slot, which the kernel boots from.  With
// HOST_PHARO set, K31 also runs the kernel in that native VM, with
// NOTEBOOK_EVENTS=/dev/fd/3.
//
// NB_CASES=<regexp> runs only the cases whose names match it.
// The cases follow the plan's tests (K1-K31, F1-F3, C1-C23); they branch on
// hello.major only where Pharo 12 and 15 really differ.  Prints every case
// and their count, and exits with status 1 if any fails.  Lane 61
// (tests/wasm/lanes/61-notebook-harness.sh) runs it once in default node
// and once with "--liftoff-only --stack-size=900", for every image.

'use strict';
const childProcess = require('child_process');
const fs = require('fs');
const os = require('os');
const path = require('path');
const zlib = require('zlib');

const webDir = path.resolve(process.argv[2] || 'build-wasm/web');
const imageDir = path.resolve(process.argv[3] || path.join(webDir, '..', 'image', 'stock'));
const srcDir = path.join(__dirname, '..', '..');
const srcWeb = path.join(srcDir, 'packaging', 'emscripten', 'web');
const createPharoVM = require(path.join(webDir, 'pharo-web.js'));
const Driver = require(path.join(srcWeb, 'vm-driver.js'));
const Kernel = require(path.join(srcWeb, 'nb-kernel.js'));
const PharoStorage = require(path.join(webDir, 'vm-storage.js'));
const WorkerShim = require('./lib/worker-shim.js');
const { WAITING, BUSY, SLEEPING, EXITED, HOST_IMAGE_SAVED } = Driver;
const wasmModule = new WebAssembly.Module(fs.readFileSync(path.join(webDir, 'pharo-web.wasm')));
const stDir = path.join(srcDir, 'packaging', 'emscripten', 'st');
const kernelSource = fs.readFileSync(path.join(stDir, 'web-notebook.st'));
const replSource = fs.readFileSync(path.join(stDir, 'web-repl.st'));
const manifest = JSON.parse(fs.readFileSync(path.join(webDir, 'manifest.json'), 'utf8'));
const ENGINE_ERRORS = /RuntimeError|RangeError|Aborted|unreachable|signature_mismatch|unsupported syscall/;
const scratch = fs.mkdtempSync(path.join(process.env.TEST_DIR || os.tmpdir(), 'notebook-harness-'));

const only = (dir, ext) => {
  const names = fs.readdirSync(dir).filter(f => f.endsWith(ext));
  if (names.length !== 1) throw new Error(`expected one ${ext} in ${dir}, found ${names.length}`);
  return names[0];
};
const imageName = only(imageDir, '.image');
const stock = {
  image: fs.readFileSync(path.join(imageDir, imageName)),
  changes: fs.readFileSync(path.join(imageDir, imageName.replace(/\.image$/, '.changes'))),
  sourcesName: only(imageDir, '.sources'),
};
stock.sources = fs.readFileSync(path.join(imageDir, stock.sourcesName));
// the image of IMAGE_DIR is of another Pharo than the site's
const otherPharo = !manifest.files.some(f => f.path === stock.sourcesName);

// The files of the kernel, in MEMFS: the image, web-notebook.st and the
// placeholders of the libraries of the FFI, as the worker gives them
function kernelFiles(image = stock.image, changes = stock.changes) {
  return [
    { path: '/pharo/Pharo.image', data: image },
    { path: '/pharo/Pharo.changes', data: changes },
    { path: '/pharo/' + stock.sourcesName, data: stock.sources },
    { path: '/pharo/st/web-notebook.st', data: kernelSource },
    ...(manifest.libraries || []).map(name => ({ path: '/pharo/' + name, data: '' })),
  ];
}
// and those of the Console's REPL
function consoleFiles(image = stock.image, changes = stock.changes) {
  return kernelFiles(image, changes).concat([{ path: '/pharo/st/web-repl.st', data: replSource }]);
}

// current: the session whose traffic a failing case shows, home at the
// start of every case (K in Part K, C in Part C)
let failures = 0, passes = 0, current = null, home = null;
const sleep = ms => new Promise(r => setTimeout(r, ms));
const now = () => performance.now();
function assert(c, msg) { if (!c) throw new Error('assertion failed: ' + msg); }
const J = x => JSON.stringify(x);
function eq(a, b, what) { assert(J(a) === J(b), what + ': expected ' + J(b) + ', got ' + J(a)); }
const utf8 = s => new TextEncoder().encode(s);
// a promise that must settle within ms
const within = (p, ms, what) => Promise.race([p, sleep(ms).then(() => { throw new Error(what + ': not within ' + ms + ' ms'); })]);

async function waitFor(what, pred, ms = 30000) {
  const t0 = now();
  for (;;) {
    const v = pred();
    if (v) return v;
    if (now() - t0 > ms) throw new Error('timed out after ' + ms + ' ms waiting for ' + what);
    await sleep(5);
  }
}

// NB_CASES, a regular expression, runs only the cases whose names match it
// (and K1 and C1, which start the kernel and the client that others use)
const selected = process.env.NB_CASES ? new RegExp(process.env.NB_CASES) : null;
async function check(name, f) {
  if (selected && !selected.test(name) && !/^(K1 hello|C1 )/.test(name)) return;
  current = home;
  try {
    await f();
    passes++;
    console.log('ok - ' + name);
  } catch (e) {
    failures++;
    console.log('not ok - ' + name + '\n  ' + String((e && e.message) || e));
    if (current && current.tail) console.log('  recent traffic:\n  | ' + current.tail().split('\n').join('\n  | '));
    if (current && current.crash && !current.crashReported) reportCrash(current);
  }
}
// The first failing case of a kernel that crashed writes all it has of the
// crash into TEST_DIR (the directory the lane runs in): the message, the JS
// stack, the Smalltalk stacks, what the runtime printed (printErr) and fd 2,
// whose last lines are what recent traffic shows; and prints the lines of
// fd 2 and of the runtime that say what failed
let crashes = 0;
function reportCrash(s) {
  s.crashReported = true;
  const file = path.join(process.env.TEST_DIR || process.cwd(), 'notebook-crash-' + process.pid + '-' + (++crashes) + '.txt');
  const report = ['crash: ' + s.crash, '--- JS stack', s.crashStack, '--- Smalltalk stacks', s.crashStacks,
                  '--- printErr', s.diag, '--- fd 2', s.output(2)].join('\n');
  try { fs.writeFileSync(file, report); console.log('  crash report: ' + file); } catch (e) { console.log('  crash report not written: ' + e.message); }
  const said = (s.diag + '\n' + s.output(2)).split('\n').filter(l => /ERROR|[Ee]rror|Abort|abort|signal|memory/.test(l)).slice(-20);
  if (said.length) console.log('  what failed:\n  | ' + said.join('\n  | '));
  const js = String(s.crashStack || '').split('\n').slice(0, 25);
  if (js.length) console.log('  JS stack:\n  | ' + js.join('\n  | '));
}
const skip = (name, why) => console.log('# skip ' + name + ': ' + why);

// The image prints some things differently (only 12 and 15 are tested):
// set from hello.major
let major = 12;
const newer = () => major > 12;

// A PNG decoded: {width, height, rgba}; checks its signature, the CRC of
// every chunk and its filters (bgraToPng writes filter 0)
function decodePng(png) {
  const b = Buffer.from(png);
  assert(b.subarray(0, 8).equals(Buffer.from([137, 80, 78, 71, 13, 10, 26, 10])), 'a PNG signature');
  let at = 8, width = 0, height = 0;
  const idat = [];
  while (at < b.length) {
    const len = b.readUInt32BE(at), type = b.toString('latin1', at + 4, at + 8);
    const crc = b.readUInt32BE(at + 8 + len);
    assert(Kernel.crc32(b.subarray(at + 4, at + 8 + len)) === crc, 'the CRC of ' + type);
    if (type === 'IHDR') {
      width = b.readUInt32BE(at + 8);
      height = b.readUInt32BE(at + 12);
      assert(b[at + 16] === 8 && b[at + 17] === 6, 'RGBA of 8 bits');
    }
    if (type === 'IDAT') idat.push(b.subarray(at + 8, at + 8 + len));
    at += 12 + len;
  }
  const raw = zlib.inflateSync(Buffer.concat(idat)), rgba = Buffer.alloc(4 * width * height);
  assert(raw.length === (4 * width + 1) * height, 'the size of the pixels');
  for (let y = 0; y < height; y++) {
    assert(raw[y * (4 * width + 1)] === 0, 'filter 0 on row ' + y);
    raw.copy(rgba, 4 * width * y, y * (4 * width + 1) + 1, (y + 1) * (4 * width + 1));
  }
  return { width, height, rgba };
}
const pixel = (img, i) => Array.from(img.rgba.subarray(4 * i, 4 * i + 4));
const near = (a, b) => a.length === b.length && a.every((x, i) => Math.abs(x - b[i]) <= 1);


// ---- Part K: the raw kernel through vm-driver.js

// One kernel.  `items' is everything it emitted, in order: {fd, text} for
// fds 1 and 2 (adjacent chunks merged) and {ev} for each event (its
// attachment as ev.attachment).  `protocol' collects the errors of the
// FrameReader, which fail the case (s.alive) unless allowProtocol.
const kernels = [];
async function kernel(opts = {}) {
  const s = { items: [], states: [], exit: null, crash: null, rid: 0, unacked: 0, high: Infinity,
              onEvent: null, protocol: [], allowProtocol: false, host: [], diag: '', badUtf8: 0,
              crashExpected: false };
  const dec = { 1: new TextDecoder(), 2: new TextDecoder() };
  const strict = { 1: new TextDecoder('utf-8', { fatal: true }), 2: new TextDecoder('utf-8', { fatal: true }) };
  const reader = new Kernel.FrameReader((ev, att) => {
    if (att) ev.attachment = att;
    s.items.push({ ev });
    if (s.onEvent) s.onEvent(ev);
  }, text => s.protocol.push(String(text)));
  s.drv = await Driver.start(createPharoVM, {
    args: opts.args || Driver.vmArgs('notebook'),
    events: opts.events !== false,
    files: opts.files || kernelFiles(),
    sliceMs: opts.sliceMs,
    wasmModule,
    schedule: f => setImmediate(f),
    later: (f, ms) => setTimeout(f, ms),
    canRun: () => s.unacked < s.high,
    onOutput: (fd, bytes) => {
      s.unacked += bytes.length;
      if (fd === 3) { reader.push(bytes); return; }
      // what is written on fds 1 and 2 must be UTF-8
      try { strict[fd].decode(bytes, { stream: true }); } catch (e) {
        s.badUtf8++;
        strict[fd] = new TextDecoder('utf-8', { fatal: true });
      }
      const text = dec[fd].decode(bytes, { stream: true });
      const last = s.items[s.items.length - 1];
      if (last && last.fd === fd) last.text += text; else s.items.push({ fd, text });
    },
    onState: st => s.states.push(st),
    onHost: (kind, text) => s.host.push({ kind, text }),
    onExit: code => { s.exit = code; },
    onCrash: (msg, stack, stacks) => { s.crash = msg; s.crashStack = stack; s.crashStacks = stacks; },
    onDiag: t => { s.diag += t; },
  });
  current = s;
  kernels.push(s);
  s.tail = () => J(s.items.slice(-8).map(i => i.ev ? Object.assign({}, i.ev, { attachment: undefined }) : i))
    .slice(-1500) + (s.protocol.length ? '\nprotocol: ' + J(s.protocol.slice(-3)) : '');
  s.events = () => s.items.filter(i => i.ev).map(i => i.ev);
  s.find = pred => s.events().find(pred);
  s.output = fd => s.items.filter(i => i.fd === fd).map(i => i.text).join('');
  s.alive = () => {
    if (s.crash) throw new Error('crash: ' + s.crash);
    if (s.exit !== null) throw new Error('exit ' + s.exit);
    if (s.protocol.length && !s.allowProtocol) throw new Error('protocol error: ' + s.protocol[0]);
  };
  s.request = req => s.drv.feed(JSON.stringify(req) + '\n');
  // post a cell; s.result(rid) resolves with {done, items (between start
  // and done), out, err, displays, events}
  s.post = (code, rid) => {
    rid = rid || ++s.rid;
    s.request({ op: 'run', rid, name: 'In[' + rid + ']', code });
    return rid;
  };
  s.result = async (rid, ms) => {
    const done = await waitFor('done ' + rid, () => { s.alive(); return s.find(e => e.ev === 'done' && e.rid === rid); }, ms);
    const i0 = s.items.findIndex(i => i.ev && i.ev.ev === 'start' && i.ev.rid === rid);
    const i1 = s.items.findIndex(i => i.ev === done);
    assert(i0 >= 0 && i0 < i1, 'start ' + rid + ' before its done');
    const items = s.items.slice(i0 + 1, i1);
    const text = fd => items.filter(i => i.fd === fd).map(i => i.text).join('');
    const evs = items.filter(i => i.ev).map(i => i.ev);
    return { done, items, out: text(1), err: text(2), events: evs, displays: evs.filter(e => e.ev === 'display') };
  };
  s.run = (code, ms) => s.result(s.post(code), ms);
  // the kernel waits for its next request
  s.idle = ms => waitFor('the kernel idle', () => {
    s.alive();
    const evs = s.events(), last = evs[evs.length - 1];
    return last && last.ev !== 'start' && s.drv.state() === WAITING;
  }, ms);
  s.stateSeen = (st, from, ms) => waitFor('state ' + st, () => { s.alive(); return s.states.slice(from).includes(st); }, ms);
  s.hello = () => waitFor('hello', () => { s.alive(); return s.find(e => e.ev === 'hello'); }, 60000);
  s.ended = ms => waitFor('exit or crash', () => s.exit !== null || s.crash, ms);
  s.close = async () => { if (s.exit === null && !s.crash) { s.drv.eof(); await s.ended(30000).catch(() => {}); } };
  if (s.drv) s.drv.begin();
  return s;
}

async function ok(s, code, values, ms) {
  const r = await s.run(code, ms);
  assert(r.done.status === 'ok', J(code).slice(0, 120) + ': status ' + r.done.status + ' ' + J(r.done.error));
  if (values !== undefined) eq(r.done.values, values, J(code).slice(0, 120));
  return r;
}
async function failing(s, code, status = 'error', ms) {
  const r = await s.run(code, ms);
  assert(r.done.status === status, J(code).slice(0, 120) + ': status ' + r.done.status + ', not ' + status +
         ' ' + J(r.done.error || r.done.values));
  return r;
}

// a class of the tests, defined in a cell
const defineClass = (name, superclass = 'Object') => `((${superclass} << #${name}) package: 'NbHarness') install`;

let K = null;                           // the kernel of most cases
let slotImage = null;                   // the image of K22, saved by the Console
let afterSnapshot = null;               // the image that K30's cell saved

async function partK() {
  await check('K1 hello {proto 1, major, version} comes first; nothing on fds 1 and 2; the .changes grows < 1 KB', async () => {
    const t0 = now();
    K = home = await kernel();
    assert(K.drv, 'start answered null: ' + K.crash);
    const h = await K.hello();
    eq(h.proto, 1, 'proto');
    assert(h.major === 12 || h.major === 15, 'major ' + h.major);
    assert(/^1[25]\.\d+$/.test(h.version) && h.version.startsWith(h.major + '.'), 'version ' + h.version);
    eq(h.wordSize, 8, 'wordSize');
    major = h.major;
    await K.idle();
    assert(K.items[0].ev === h, 'hello first: ' + K.tail());
    assert(!K.items.some(i => i.fd), 'no output on fds 1 and 2: ' + J(K.items.filter(i => i.fd)));
    const grown = K.drv.FS.stat('/pharo/Pharo.changes').size - stock.changes.length;
    assert(grown < 1024, `the .changes grew by ${grown} bytes`);
    console.log(`#   Pharo ${h.version} (${h.image}), hello after ${(now() - t0).toFixed(0)} ms, the .changes grew by ${grown} bytes`);
  });
  if (!K || !K.drv || K.exit !== null || K.crash || !K.find(e => e.ev === 'hello')) { console.log('cannot continue'); return false; }

  await check('K1 negative: vmArgs(console) prompts st> and sends no hello; a Console file-in of web-notebook.st keeps the REPL', async () => {
    const C = await kernel({ args: Driver.vmArgs('console'), files: consoleFiles() });
    try {
      await waitFor('the prompt', () => { C.alive(); return /st> $/.test(C.output(1)); }, 60000);
      await sleep(500);
      eq(C.output(1), 'st> ', 'the output');
      eq(C.events(), [], 'events');
      // without NOTEBOOK_EVENTS (no events device), filing it in only
      // defines the classes
      const D = await kernel({ args: Driver.vmArgs('console'), files: consoleFiles(), events: false });
      current = D;
      try {
        await waitFor('the prompt', () => { D.alive(); return /st> $/.test(D.output(1)); }, 60000);
        D.drv.feed("'/pharo/st/web-notebook.st' asFileReference fileIn. (Smalltalk hasClassNamed: #NotebookKernel) -> (3 + 4)\n");
        await waitFor('the result', () => { D.alive(); return /true->7\nst> $/.test(D.output(1)); }, 60000);
        D.drv.feed('6 * 7\n');
        await waitFor('the REPL', () => { D.alive(); return /42\nst> $/.test(D.output(1)); });
        eq(D.events(), [], 'events');
      } finally { await D.close(); }
    } finally { await C.close(); }
  });

  await check('K2 stdout, stderr and Transcript in order between start and done; values; done last with ms', async () => {
    const r = await ok(K, "Stdio stdout nextPutAll: 'a'; flush. Stdio stderr nextPutAll: 'b'; flush. Transcript show: 'c'; flush. 42", ['42']);
    eq(r.items.map(i => i.ev ? i.ev.ev : i.fd + ':' + i.text), ['1:a', '2:b', '1:c'], 'items between start and done');
    assert(K.items[K.items.length - 1].ev === r.done, 'done last');
    assert(Number.isInteger(r.done.ms) && r.done.ms >= 0 && r.done.rid === K.rid, 'ms and rid: ' + J(r.done));
    eq(K.find(e => e.ev === 'start' && e.rid === K.rid), { ev: 'start', rid: K.rid, name: 'In[' + K.rid + ']' }, 'start');
  });

  await check('K3 variables, blocks, classes and globals persist across cells; an undeclared read is form 2', async () => {
    await ok(K, 'k3a := 5', ['5']);
    await ok(K, 'k3a + 1', ['6']);
    await ok(K, 'k3blk := [ k3a * 2 ]. 0', ['0']);
    await ok(K, 'k3a := 10', ['10']);
    await ok(K, 'k3blk value', ['20']);
    await ok(K, defineClass('NbK3') + ". NbK3 compile: 'k3 ^ 33'. #defined", ['#defined']);
    await ok(K, 'NbK3 new k3', ['33']);
    await ok(K, defineClass('NbK3b') + ". NbK3b compile: 'v ^ 7'. NbK3b new v", ['7']);
    await ok(K, "Object compile: 'nbK3Probe ^ 3' classified: 'nbharness'. Smalltalk at: #NbK3G put: 9. 0", ['0']);
    await ok(K, '(Smalltalk at: #NbK3G) + nil nbK3Probe', ['12']);
    const r = await failing(K, 'k3n := 1.\nNbSmae new');
    eq([r.done.error.form, r.done.error.line], [2, 2], 'form and line');
    assert(/Undeclared variable/.test(r.done.error.text), 'text ' + r.done.error.text);
    await ok(K, 'k3n', ['1']);
  });

  await check('K4 values: printStrings, null, assignments, temporaries, ^ anywhere ends the cell, a cascade', async () => {
    await ok(K, '3 + 4', ['7']);
    for (const empty of ['nil', '', '   \n ', '"just a comment"', '| t |']) await ok(K, empty, null);
    await ok(K, '3 + 4.', ['7']);
    await ok(K, 'k4 := 6 * 7', ['42']);
    await ok(K, '| t4 | t4 := 3. t4 + k4', ['45']);
    await failing(K, 't4');
    await ok(K, '^ 5. Smalltalk at: #NbK4six put: 6', ['5']);
    await ok(K, 'Smalltalk includesKey: #NbK4six', ['false']);
    await ok(K, 'true ifTrue: [ ^ 1 ]. 2', ['1']);
    await ok(K, '#(1 2) do: [ :e | ^ e ]. Smalltalk at: #NbMark put: 1', ['1']);
    await ok(K, 'Smalltalk includesKey: #NbMark', ['false']);
    await ok(K, 'OrderedCollection new add: 3; add: 4; yourself', ['an OrderedCollection(3 4)']);
    await ok(K, 'k4b := 1. k4b := k4b + 1. ^ k4b', ['2']);
  });

  await check('K5 a syntax error evaluates nothing: status syntax, line and column; complete edge cases evaluate', async () => {
    for (const code of ["Transcript show: 'x'. 'abc", 'Transcript show: \'x\'. "abc', 'Transcript show: \'x\'. [ 1',
                        "Transcript show: 'x'. #(1 2", "Transcript show: 'x'. { 1. 2", "Transcript show: 'x'. 1 ]",
                        "Transcript show: 'x'. 1 )"]) {
      const r = await failing(K, code, 'syntax');
      const e = r.done.error;
      assert(/^Syntax error: .* \(line \d+, column \d+\)$/.test(e.text), 'text ' + J(e.text));
      assert(Number.isInteger(e.line) && Number.isInteger(e.column) && e.form === null, J(code) + ': ' + J(e));
      eq(e.kind, ['syntax'], 'kind');
      eq(e.chain, [], 'chain');
      eq([r.out, r.err], ['', ''], J(code) + ' wrote');
    }
    for (const nl of ['\n', '\r', '\r\n']) {
      const r = await failing(K, ['1.', '2.', '3 +'].join(nl), 'syntax');
      eq(r.done.error.line, 3, 'the line with ' + J(nl));
    }
    await failing(K, 'y5 := 1. 3 +', 'syntax');
    await failing(K, 'y5');
    const p = await failing(K, '<primitive: 1> 3', 'syntax');
    assert(/pragmas are not allowed/.test(p.done.error.text), 'pragma: ' + p.done.error.text);
    for (const [code, value] of [['3 "c"', '3'], ["$' printString", "'$'''"], ['#($) $] 2) size', '3'],
                                 ["'it''s'", "'it''s'"], ['#[1 2 255]', '#[1 2 255]'], ['2r101 + 16r1F', '36'],
                                 ['1.5s2', '1.50s2'], ['#at:put: numArgs', '2'], ['"a [ ( comment" 1', '1']])
      await ok(K, code, [value]);
    const s = await failing(K, 'self := 3');
    eq(s.done.error.form, 1, 'self := 3, form');
  });

  await check('K6 a runtime error: text, kind, location, form, line, column, the chain outermost first; Notebook lastError', async () => {
    await ok(K, defineClass('NbErr') + ". NbErr compile: 'f: x ^ x foo'. NbErr compile: 'g: x ^ self f: x'. 0", ['0']);
    const r = await failing(K, '1 + 1.\nNbErr new g: nil');
    const e = r.done.error;
    assert(/^Error: MessageNotUnderstood/.test(e.text) && /foo/.test(e.text), 'text ' + e.text);
    assert(e.kind[0] === 'MessageNotUnderstood' && e.kind[e.kind.length - 1] === 'Exception', 'kind ' + J(e.kind));
    assert(typeof e.location === 'string' && e.location.length, 'location ' + J(e.location));
    eq([e.form, e.line], [2, 2], 'form and line');
    const where = e.chain.map(f => f.where);
    eq(where[0], 'In[' + r.done.rid + ']:2', 'the outermost frame');
    const gi = where.indexOf('NbErr>>g:'), fi = where.indexOf('NbErr>>f:');
    assert(gi > 0 && fi > gi, 'NbErr>>g:, then NbErr>>f: ' + J(where));
    assert(e.chain.length <= 20, 'at most 20 frames');
    // (Pharo 15 keeps the frame of Object>>doesNotUnderstand:, whose
    // source names handleDoesNotUnderstand:to:, but not that of Symbol's)
    assert(!e.chain.some(f => /NotebookKernel|handleDoesNotUnderstand/.test(f.where + ' ' + f.proc)), 'no kernel frames: ' + J(e.chain));
    assert(e.chain[0].proc === null && /NbErr new g: nil/.test(e.chain[0].form), 'the DoIt frame ' + J(e.chain[0]));
    // the column, in a statement that the kernel wraps
    const c = await failing(K, '1.\n   nil bar');
    eq([c.done.error.line, c.done.error.column], [2, 4], 'line and column of nil bar');
    await ok(K, 'Notebook lastError class name', ['#MessageNotUnderstood']);
  });

  await check('K7 the statements before the error ran; ZeroDivide with its empty messageText', async () => {
    const r = await failing(K, 'a7 := 1. 1/0. a7 := 2');
    assert(/^Error: ZeroDivide\s*$/.test(r.done.error.text), 'text ' + J(r.done.error.text));
    eq(r.done.error.form, 2, 'form');
    await ok(K, 'a7', ['1']);
  });

  await check('K7b an error in the 3rd chunk of a .st filed in: the form and line of the cell', async () => {
    await ok(K, "'/pharo/k7b.st' asFileReference ensureDelete; writeStreamDo: [ :s | s nextPutAll: '1 + 1!'; lf; " +
                "nextPutAll: '2 + 2!'; lf; nextPutAll: 'nil k7bFoo!'; lf ]. 0", ['0']);
    const r = await failing(K, "1.\n'/pharo/k7b.st' asFileReference fileIn");
    eq([r.done.error.form, r.done.error.line], [2, 2], 'form and line');
    assert(/k7bFoo/.test(r.done.error.text), 'text ' + r.done.error.text);
  });

  await check('K8 errors and warnings: signals, error:, Exception, Halt; a Warning, a Notification; print errors; truncation; the fallback', async () => {
    for (const code of ['Error new signal', "self error: 'k8'", 'Exception signal', 'Halt now']) await failing(K, code);
    await ok(K, '3', ['3']);
    const w = await ok(K, "Warning signal: 'careful'. 'went on'", ["'went on'"]);
    assert(/careful/.test(w.err), 'the warning on stderr: ' + J(w.err));
    const n = await ok(K, "Notification signal: 'quiet'. 5", ['5']);
    eq([n.out, n.err, n.displays], ['', '', []], 'nothing shown');
    await ok(K, defineClass('NbK8P') + ". NbK8P compile: 'printOn: s self error: ''nope'''. 0", ['0']);
    const p = await failing(K, 'NbK8P new');
    eq(p.done.error.form, 'print', 'form');
    const t = await ok(K, '(1 to: 100000) asArray');
    assert(t.done.truncated === true && t.done.values[0].length <= 65536 + 16 && /^#\(1 2 3 /.test(t.done.values[0]),
           'truncated: ' + t.done.values[0].length + ' ' + t.done.truncated);
    await ok(K, defineClass('NbK8Err', 'Error') + ". NbK8Err compile: 'messageText ^ nil nbNoSuchThing'. " +
                "NbK8Err compile: 'description ^ nil nbNoSuchThing'. 0", ['0']);
    const f = await failing(K, 'NbK8Err new signal');
    eq(f.done.error, { text: 'Error: an error occurred while reporting an error', kind: [], location: null, form: null,
                       line: null, column: null, chain: [] }, 'the fallback');
    await ok(K, '4', ['4']);
  });

  await check('K9 Stop within 2 s of a loop, a counted loop, a Delay, a wait, a read of stdin; ensure: blocks print before done', async () => {
    for (const [code, st] of [['[ true ] whileTrue', BUSY], ['1 to: SmallInteger maxVal do: [ :i | ]', BUSY],
                              ['(Delay forSeconds: 10) wait', SLEEPING], ['Semaphore new wait', null],
                              ['Stdio stdin waitForData', null],
                              ["[ (Delay forSeconds: 10) wait ] ensure: [ Transcript show: 'ens'; flush ]", SLEEPING]]) {
      const from = K.states.length;
      const rid = K.post(code);
      await waitFor('start', () => K.find(e => e.ev === 'start' && e.rid === rid));
      if (st !== null) await K.stateSeen(st, from);
      if (st === BUSY) await waitFor('two BUSY', () => K.states.slice(from).filter(x => x === BUSY).length >= 2);
      await sleep(200);
      const t = now();
      assert(K.drv.interrupt() === true, 'interrupt() answers true');
      const r = await K.result(rid, 2000);
      eq(r.done.status, 'interrupted', code);
      assert(now() - t < 2000, 'within 2 s');
      if (/ens/.test(code)) eq(r.out, 'ens', 'the ensure: block');
      await ok(K, '6 * 7', ['42']);
    }
  });

  await check('K10 stale Stops are dropped: one in the slice of a done, one while idle; the kernel does not exit', async () => {
    // (inside the slice, where interrupt() may not be called: the semaphore
    // itself, as vm-harness.js case 13b does)
    let signalled = 0;
    K.onEvent = ev => { if (ev.ev === 'done') { K.onEvent = null; signalled = K.drv.module._vm_interrupt(); } };
    await ok(K, '1 + 2', ['3']);
    eq(signalled, 1, 'the semaphore signalled in the slice of the done');
    await K.idle();
    await ok(K, '1 + 1', ['2']);
    await K.idle();
    assert(K.drv.interrupt() === true, 'interrupt() answers true');
    await sleep(300);
    await ok(K, '(1 to: 200000) inject: 0 into: [ :a :b | a + 1 ]', ['200000']);
    await sleep(300);
    K.alive();
  });

  await check('K11 a request and Stop in the same turn: start, then done interrupted, nothing run', async () => {
    await K.idle();
    const rid = K.post('Smalltalk at: #NbK11 put: 1. [ true ] whileTrue');
    K.drv.interrupt();
    const r = await K.result(rid, 5000);
    eq(r.done.status, 'interrupted', 'status');
    await ok(K, 'Smalltalk includesKey: #NbK11', ['false']);
  });

  // vm_interrupt() signals the semaphore between slices; the watcher must
  // take it in the slice that follows, while stdin is still empty, and not
  // at the next timer or input, which may be the next request
  await check('K11c a Stop while the kernel is idle does nothing: a run 2, 5, 15 or 30 ms after it is ok (10 trials each)', async () => {
    const statuses = {};
    for (const ms of [2, 5, 15, 30]) {
      for (let t = 0; t < 10; t++) {
        await K.idle();
        await sleep(20 + Math.random() * 40);
        assert(K.drv.interrupt() === true, 'interrupt() answers true');
        await sleep(ms);
        const r = await K.result(K.post('3 + 4'), 5000);
        const key = ms + ' ms ' + r.done.status;
        statuses[key] = (statuses[key] || 0) + 1;
      }
    }
    eq(statuses, { '2 ms ok': 10, '5 ms ok': 10, '15 ms ok': 10, '30 ms ok': 10 }, 'the statuses');
    // and a request then Stop in the same turn is still stopped
    const rid = K.post('Smalltalk at: #NbK11c put: 1. [ true ] whileTrue');
    K.drv.interrupt();
    eq((await K.result(rid, 5000)).done.status, 'interrupted', 'a request then Stop');
    await ok(K, 'Smalltalk includesKey: #NbK11c', ['false']);
  });

  await check('K13 5 MB under the credit of canRun (1 MiB): complete, in order, done right after', async () => {
    K.high = 1 << 20;
    K.unacked = 0;
    const timer = setInterval(() => { K.unacked = 0; K.drv.resumeOutput(); }, 20);
    try {
      const r = await ok(K, '1 to: 50000 do: [ :i | Stdio stdout nextPutAll: (String new: 99 withAll: $x); lf ]. nil', null, 120000);
      assert(r.out === ('x'.repeat(99) + '\n').repeat(50000), 'the output: ' + r.out.length + ' characters');
      const i1 = K.items.findIndex(i => i.ev === r.done);
      assert(K.items[i1 - 1].fd === 1, 'done follows the last stdout');
    } finally { clearInterval(timer); K.high = Infinity; }
  });

  await check('K14 re-entry: a block of an earlier cell returns into nothing; a kept exception or context cannot resume', async () => {
    await ok(K, 'blk14 := [ :x | ^ x ]. 0', ['0']);
    const r = await failing(K, 'blk14 value: 3');
    assert(r.done.error.kind.includes('BlockCannotReturn'), 'kind ' + J(r.done.error.kind));
    eq(r.done.rid, K.rid, 'reported to the later cell');
    await ok(K, 'k14ex := [ 1/0 ] on: ZeroDivide do: [ :e | e ]. k14ctx := thisContext. 0', ['0']);
    for (const code of ['k14ex retry', 'k14ex resume: 5', 'k14ctx resume: 5']) {
      const x = await K.run(code);
      assert(x.done.status === 'error', J(code) + ': status ' + x.done.status + ' ' + J(x.done.values));
      await ok(K, '3 + 4', ['7']);
    }
  });

  await check('K15 a cell that terminates its process: terminated, with what it wrote before', async () => {
    const r = await failing(K, "Transcript show: 'before'; flush. Processor activeProcess terminate. Transcript show: 'after'", 'terminated');
    eq(r.out, 'before', 'stdout');
    await ok(K, '1 + 1', ['2']);
  });

  await check('K16 requests in order; ping; bad requests (cut to 200 characters); the kernel goes on', async () => {
    await K.idle();
    const a = K.post('(Delay forMilliseconds: 100) wait. 1'), b = K.post('2');
    const ra = await K.result(a), rb = await K.result(b);
    eq([ra.done.values, rb.done.values], [['1'], ['2']], 'values');
    assert(K.items.findIndex(i => i.ev === ra.done) < K.items.findIndex(i => i.ev === rb.done), 'in order');
    K.request({ op: 'ping', rid: 77 });
    await waitFor('pong', () => K.find(e => e.ev === 'pong' && e.rid === 77));
    const bad = ['hello', '{"op":"frob","rid":5}', '{"op":"run","rid":0,"name":"In[0]","code":"1"}',
                 '{"op":"run","rid":"x","name":"In[1]","code":"1"}', '{"op":"run","rid":1.0,"name":"In[1]","code":"1"}',
                 '{"op":"run","rid":1e400,"name":"In[1]","code":"1"}', '{"op":"run","rid":1073741824,"name":"In[1]","code":"1"}',
                 '{"op":"run","rid":Point[1,2],"name":"In[1]","code":"1"}', '{"rid":Point[1,2]}', '#run',
                 "{'op':'ping','rid':1}", '{"op":"ping","rid":1} x', '{"op":"run","rid":5,"name":"In 5","code":"1"}',
                 '{"op":"run","rid":5,"name":"In[5]"}', '{"op":"run","rid":5,"name":"In[5]","code":3}',
                 '{"op":"ping"}', '[1,2]', 'x'.repeat(1000)];
    // each one, then a ping: exactly a bad-request comes before its pong
    const wrong = [];
    for (const [i, line] of bad.entries()) {
      const n = K.events().length;
      K.drv.feed(line + '\n');
      K.request({ op: 'ping', rid: 1000 + i });
      await waitFor('pong ' + (1000 + i), () => { K.alive(); return K.find(e => e.ev === 'pong' && e.rid === 1000 + i); });
      const got = K.events().slice(n, -1);
      if (J(got) !== J([{ ev: 'bad-request', text: line.slice(0, 200) }])) wrong.push(line.slice(0, 40) + ' -> ' + J(got).slice(0, 120));
    }
    eq(wrong, [], 'the answers to bad requests');
    await ok(K, '3 + 4', ['7']);
  });

  await check('K17 the kernel refuses to start twice', async () => {
    const r = await failing(K, 'NotebookKernel start');
    assert(/already running/.test(r.done.error.text), J(r.done.error.text));
    await ok(K, '1 + 1', ['2']);
  });

  await check('K18 UTF-8, NUL, the boundaries of reads and writes, JSON escapes; surrogates and > 10FFFF become U+FFFD', async () => {
    const big = '\u00E9\u2713\u{1D11E}';
    await ok(K, "'h" + big + "'", ["'h" + big + "'"]);
    await ok(K, "'h" + big + "' size", ['4']);
    await ok(K, '(String with: $a with: (Character value: 0)) size', ['2']);
    await ok(K, "'a\u0000b' size", ['3']);
    await ok(K, 'String with: $a with: (Character value: 0)', ["'a\u0000'"]);
    for (const n of [4095, 4096, 8191, 65535, 65536]) {
      const r = await ok(K, `Transcript show: (String new: ${n} withAll: $a); show: '${big}'; flush. ` +
                            `Notebook show: (Notebook text: (String new: ${n} withAll: $b), '${big}'). 0`, ['0']);
      assert(r.out === 'a'.repeat(n) + big, 'the output across ' + n);
      assert(r.displays.length === 1 && r.displays[0].data === 'b'.repeat(n) + big, 'the display across ' + n);
    }
    const raw = '"\\\n\t\r\u0001\u001f</script> ';
    const r = await ok(K, 'Notebook show: (Notebook text: (String withAll: (#(' + [...raw].map(c => c.codePointAt(0)).join(' ') +
                          ') collect: [ :c | Character value: c ]))). 0', ['0']);
    eq(r.displays.map(d => d.data), [raw], 'the escapes round-trip');
    // M2: a lone surrogate and a code point above 10FFFF
    const bad = '(String with: $a with: (Character value: 16rD800) with: (Character value: 16r110000))';
    await ok(K, bad, ["'a\uFFFD\uFFFD'"]);
    const t = await ok(K, `Transcript show: ${bad}; flush. 1`, ['1']);
    eq(t.out, 'a\uFFFD\uFFFD', 'Transcript');
    const d = await ok(K, `Notebook show: (Notebook text: ${bad}). 1`, ['1']);
    eq(d.displays.map(x => x.data), ['a\uFFFD\uFFFD'], 'display');
    const f = await ok(K, `[ (Delay forMilliseconds: 50) wait. Transcript show: ${bad}; flush ] fork. 1`, ['1']);
    await ok(K, '(Delay forMilliseconds: 400) wait. 2', ['2']);
    const st = K.find(e => e.ev === 'stream' && e.rid === f.done.rid);
    assert(st && st.text === 'a\uFFFD\uFFFD' && st.name === 'stdout', 'stream ' + J(st));
    eq(K.badUtf8, 0, 'writes on fds 1 and 2 that are not UTF-8');
    await ok(K, '3 + 4', ['7']);
  });

  await check('K20 the Notebook API: show:id:, svg, html, text, table, markdown, png:, clearOutput, a Form, an empty Form, isDisplay:', async () => {
    let r = await ok(K, "Notebook show: (Notebook html: '<b>x</b>') id: 'p'. Notebook show: (Notebook html: '<b>y</b>') id: 'p'. " +
                        "Notebook svg: '<svg><circle r=\"5\"/></svg>'", null);
    eq(r.displays.map(d => [d.mime, d.id, d.rid]),
       [['text/html', 'p', r.done.rid], ['text/html', 'p', r.done.rid], ['image/svg+xml', null, r.done.rid]], 'displays');
    eq(r.displays[1].data, '<b>y</b>', 'the second');
    assert(r.displays[2].data.startsWith('<svg xmlns="http://www.w3.org/2000/svg"') && /r="5"/.test(r.displays[2].data),
           'svg ' + r.displays[2].data);
    assert(K.items[K.items.findIndex(i => i.ev === r.displays[2]) + 1].ev === r.done, 'the value display just before done');
    r = await ok(K, "Notebook svg: '<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 1 1\"></svg>'", null);
    eq(r.displays[0].data.match(/xmlns=/g).length, 1, 'one xmlns');
    r = await ok(K, "Notebook text: 'a<b'", null);
    eq(r.displays.map(d => [d.mime, d.data]), [['text/plain', 'a<b']], 'text');
    r = await ok(K, 'Notebook text: 42', null);
    eq(r.displays.map(d => d.data), ['42'], 'text of a number');
    r = await ok(K, "Notebook html: '<p>a&lt;b</p>'", null);
    eq(r.displays.map(d => [d.mime, d.data]), [['text/html', '<p>a&lt;b</p>']], 'html, raw');
    r = await ok(K, "Notebook table: #(#(1 'one') #(2 '<two>')) header: #('n' 'name')", null);
    const table = r.displays[0].data.replace(/>\s+</g, '><');
    eq(r.displays[0].mime, 'text/html', 'table mime');
    assert(/^<table[ >]/.test(table) && table.endsWith('</table>') &&
           table.includes('<thead><tr><th>n</th><th>name</th></tr></thead>') &&
           table.includes('<tr><td>1</td><td>one</td></tr><tr><td>2</td><td>&lt;two&gt;</td></tr>'), 'table ' + table);
    r = await ok(K, "Notebook table: { { Notebook html: '<i>r</i>'. 'a&b \"q\"'. $< } . 7 }", null);
    const t2 = r.displays[0].data;
    assert(t2.includes('<td><i>r</i></td>') && t2.includes('<td>a&amp;b &quot;q&quot;</td>') && t2.includes('<td>&lt;</td>') &&
           t2.includes('<td>7</td>') && !t2.includes('<thead>'), 'html spliced, text escaped, a scalar row: ' + t2);
    r = await ok(K, "Notebook markdown: '# T'", null);
    eq(r.displays.map(d => [d.mime, d.data]), [['text/markdown', '# T']], 'markdown');
    r = await ok(K, 'Notebook png: #[137 80 78 71 13 10 26 10]', null);
    assert(r.displays.length === 1 && r.displays[0].mime === 'image/png' && r.displays[0].bytes === 8 &&
           Buffer.from(r.displays[0].attachment).equals(Buffer.from([137, 80, 78, 71, 13, 10, 26, 10])), 'png ' + J(r.displays[0]));
    r = await ok(K, "Notebook show: 'plain'. Notebook show: 42. Notebook clearOutput. 7", ['7']);
    eq(r.events.map(e => e.ev), ['display', 'display', 'clear'], 'events');
    eq(r.displays.map(d => [d.mime, d.data]), [['text/plain', 'plain'], ['text/plain', '42']], 'show of a String and a number');
    r = await ok(K, '| f | f := Form extent: 3 @ 1 depth: 32. f colorAt: 0 @ 0 put: Color red. ' +
                    'f colorAt: 1 @ 0 put: (Color red alpha: 0.5). f', null);
    const img = r.displays[0];
    assert(img && img.mime === 'image/x-pharo-bgra' && img.width === 3 && img.height === 1 && img.bytes === 12 &&
           img.attachment.length === 12 && img.id === null, 'the Form ' + J(Object.assign({}, img, { attachment: undefined })));
    eq(Array.from(img.attachment.subarray(0, 4)), [0, 0, 255, 255], 'red, B G R A');
    eq(img.premultiplied, false, 'a BitBlt Form is not premultiplied');
    const png = decodePng(Buffer.from(await Kernel.bgraToPng(3, 1, img.attachment, img.premultiplied), 'base64'));
    assert(png.width === 3 && near(pixel(png, 0), [255, 0, 0, 255]) && near(pixel(png, 1), [255, 0, 0, 128]),
           'decoded: ' + J([pixel(png, 0), pixel(png, 1), pixel(png, 2)]));
    // a Form without pixels is an empty image, not one too large to send
    for (const [code, text] of [['Form extent: 0 @ 0 depth: 32', 'empty image 0x0'],
                                ['Notebook show: (Form extent: 0 @ 5 depth: 1). nil', 'empty image 0x5'],
                                ['Notebook show: (Form extent: 7 @ 0 depth: 32) id: #e. nil', 'empty image 7x0']]) {
      r = await ok(K, code, null);
      eq(r.displays.map(d => [d.mime, d.data, d.size]), [['text/plain', text, undefined]], code);
    }
    await ok(K, "Notebook isDisplay: (Notebook text: '')", ['true']);
    await ok(K, 'Notebook isDisplay: 3', ['false']);
    r = await ok(K, "(Notebook show: 'x') printString", ["'nil'"]);
    r = await ok(K, "(Notebook html: '<b>x</b>') printString", ["'a NotebookDisplay(text/html, 8 chars)'"]);
  });

  await check('K21 the undeclared notices of a compile: land in that cell, not in the next', async () => {
    // (Pharo 15's compile: refuses the method: an error of that cell)
    const code = defineClass('NbX') + ". NbX compile: 'foo ^ undefinedThing'. 0";
    const r = newer() ? await failing(K, code) : await ok(K, code, ['0']);
    const said = newer() ? r.done.error.text : r.out + r.err;
    assert(/undefinedThing/.test(said), 'the notice: ' + J(said));
    if (newer()) eq(r.done.error.form, 2, 'form');
    const n = await ok(K, '1', ['1']);
    eq([n.out, n.err], ['', ''], 'the next cell');
  });

  await check('K23 a 64-bit VM; a recursion 1e6 deep; an endless recursion stopped after 1 s', async () => {
    await ok(K, 'Smalltalk vm wordSize', ['8']);
    await ok(K, defineClass('NbRec') + ". NbRec compile: 'down: n ^ n = 0 ifTrue: [ 0 ] ifFalse: [ 1 + (self down: n - 1) ]'. " +
                "NbRec compile: 'forever: n ^ 1 + (self forever: n)'. 0", ['0']);
    await ok(K, 'NbRec new down: 1000000', ['1000000'], 120000);
    const rid = K.post('NbRec new forever: 0');
    await waitFor('start', () => K.find(e => e.ev === 'start' && e.rid === rid));
    await sleep(1000);
    K.drv.interrupt();
    eq((await K.result(rid, 60000)).done.status, 'interrupted', 'status');
    await ok(K, '3 + 4', ['7']);
  });

  await check('K24 a file uploaded into /pharo files in', async () => {
    K.drv.FS.writeFile('/pharo/u.st', 'Smalltalk at: #NbUpVal put: 99!\n');
    await ok(K, "'u.st' asFileReference fileIn. Smalltalk at: #NbUpVal", ['99']);
  });

  await check('K25 vmArgs(notebook)', async () => {
    eq(Driver.vmArgs('notebook'), ['--headless', '/pharo/Pharo.image', '--no-default-preferences', 'st', '--no-source',
                                   '/pharo/st/web-notebook.st'], 'vmArgs');
  });

  await check('K28 3 MB in one nextPutAll: is complete; a display over 4 MB comes with its size only', async () => {
    const r = await ok(K, 'Stdio stdout nextPutAll: (String new: 3000000 withAll: $z). 1', ['1'], 60000);
    assert(r.out === 'z'.repeat(3000000), 'the output: ' + r.out.length);
    const d = await ok(K, 'Notebook show: (Notebook text: (String new: 4194305 withAll: $q)). 1', ['1'], 60000);
    eq(d.displays.map(x => [x.mime, x.data, x.size]), [['text/plain', '', 4194305]], 'the display');
  });

  await check('K29 background processes: streams and errors of their cell; stop-background spares higher priorities', async () => {
    const f = await ok(K, "[ (Delay forMilliseconds: 200) wait. Transcript show: 'late'; flush ] fork. 1", ['1']);
    assert(f.done.ms < 200, 'done at once: ' + f.done.ms + ' ms');
    const later = await ok(K, '(Delay forMilliseconds: 600) wait. 2', ['2']);
    eq(later.out, '', 'the running cell has none of it');
    const st = K.find(e => e.ev === 'stream' && e.rid === f.done.rid);
    assert(st && st.name === 'stdout' && st.text === 'late', 'a stream of the earlier cell: ' + J(st));
    const e = await ok(K, '[ nil k29foo ] fork. 1', ['1']);
    const err = await waitFor('error', () => { K.alive(); return K.find(x => x.ev === 'error' && x.rid === e.done.rid); }, 5000);
    assert(/^Error: MessageNotUnderstood/.test(err.error.text) && /k29foo/.test(err.error.text), 'error ' + J(err));
    await ok(K, '3 + 4', ['7']);
    await ok(K, "3 timesRepeat: [ [ [ true ] whileTrue: [ (Delay forMilliseconds: 50) wait ] ] forkNamed: 'nbbg' ]. " +
                '[ [ true ] whileTrue: [ (Delay forMilliseconds: 50) wait ] ] ' +
                "forkAt: Processor userSchedulingPriority + 1 named: 'nbhi'. 1", ['1']);
    const n = K.events().length;
    K.request({ op: 'stop-background' });
    const stopped = await waitFor('stopped', () => { K.alive(); return K.events().slice(n).find(x => x.ev === 'stopped'); });
    assert(Number.isInteger(stopped.count) && stopped.count >= 3, 'stopped ' + J(stopped));
    const alive = name => `(Process allInstances select: [ :p | p name = '${name}' and: [ p isTerminated not ] ]) size`;
    await ok(K, `{ ${alive('nbbg')}. ${alive('nbhi')} }`, ['#(0 1)']);
    await ok(K, "(Process allInstances select: [ :p | p name = 'nbhi' ]) do: [ :p | p terminate ]. 0", ['0']);
    console.log(`#   stop-background stopped ${stopped.count} processes`);
  });

  await check('K30 a snapshot in a cell ends ok, and the next cell works', async () => {
    const hosts = K.host.length;
    await ok(K, 'Smalltalk at: #NbSnapMarker put: 30. Smalltalk snapshot: true andQuit: false. 1', ['1'], 120000);
    await waitFor('onHost', () => K.host.length > hosts);
    eq(K.host[hosts].kind, HOST_IMAGE_SAVED, 'the save notified');
    afterSnapshot = { image: K.drv.FS.readFile('/pharo/Pharo.image'), changes: K.drv.FS.readFile('/pharo/Pharo.changes') };
    await ok(K, '6 * 7', ['42']);
  });

  await check('K30b after a snapshot: errors of forked processes are events, Transcript of a fork is a stream of its cell', async () => {
    const e = await ok(K, '[ 1/0 ] fork. 1', ['1']);
    const err = await waitFor('error', () => { K.alive(); return K.find(x => x.ev === 'error' && x.rid === e.done.rid); }, 5000);
    assert(/ZeroDivide/.test(err.error.text), 'error ' + J(err));
    const f = await ok(K, "[ (Delay forMilliseconds: 100) wait. Transcript show: 'x'; flush ] fork. 1", ['1']);
    await ok(K, '(Delay forMilliseconds: 500) wait. 2', ['2']);
    const st = K.find(x => x.ev === 'stream' && x.rid === f.done.rid);
    assert(st && st.text === 'x', 'stream ' + J(st));
    await ok(K, 'Smalltalk at: #NbSnapMarker', ['30']);
  });

  await check('K11b (sliceMs 1, 200 trials) a request then Stop from setImmediate: always interrupted, the marker never set', async () => {
    const X = await kernel({ sliceMs: 1 });
    try {
      await X.hello();
      await X.idle();
      const statuses = {};
      for (let i = 0; i < 200; i++) {
        const rid = X.post('(Delay forMilliseconds: 100) wait. Smalltalk at: #NbK11b put: true');
        await new Promise(r => setImmediate(() => { X.drv.interrupt(); r(); }));
        const r = await X.result(rid, 10000);
        statuses[r.done.status] = (statuses[r.done.status] || 0) + 1;
        await X.idle();
      }
      eq(statuses, { interrupted: 200 }, 'the statuses');
      await ok(X, 'Smalltalk includesKey: #NbK11b', ['false']);
    } finally { await X.close(); }
  });

  await check('K15b Transcript flush then Smalltalk exit: 3: onExit(3), no done, the output delivered', async () => {
    const X = await kernel();
    try {
      await X.hello();
      const n = X.events().length;
      X.post("Transcript show: 'bye'; flush. Smalltalk exit: 3");
      await waitFor('exit', () => X.exit !== null || X.crash, 30000);
      eq(X.exit, 3, 'the exit code');
      assert(!X.events().slice(n).some(e => e.ev === 'done'), 'no done');
      assert(/bye/.test(X.output(1)), 'bye: ' + J(X.output(1)));
    } finally { }
  });

  await check('K22 an image the Console saved (PageMarker 777): the kernel sees it, no WebRepl process is left, requests are served', async () => {
    const C = await kernel({ args: Driver.vmArgs('console'), files: consoleFiles(), events: false });
    try {
      await waitFor('the prompt', () => { C.alive(); return /st> $/.test(C.output(1)); }, 60000);
      C.drv.feed('Smalltalk at: #PageMarker put: 777. Smalltalk snapshot: true andQuit: false. #saved\n');
      await waitFor('saved', () => { C.alive(); return /#saved\nst> $/.test(C.output(1)); }, 120000);
      slotImage = { image: C.drv.FS.readFile('/pharo/Pharo.image'), changes: C.drv.FS.readFile('/pharo/Pharo.changes') };
    } finally { await C.close(); }
    const X = await kernel({ files: kernelFiles(slotImage.image, slotImage.changes) });
    try {
      const h = await X.hello();
      assert(X.items[0].ev === h, 'hello first: ' + X.tail());
      await ok(X, 'Smalltalk at: #PageMarker', ['777']);
      for (let i = 0; i < 5; i++) await ok(X, `${i} + 1`, [String(i + 1)]);
      await ok(X, "Smalltalk garbageCollect. (Process allInstances select: [ :p | p isTerminated not and: [ (p name ifNil: [ '' ]) asString beginsWith: 'WebRepl' ] ]) size", ['0']);
    } finally { await X.close(); }
  });

  await check('K22b an image saved by a running kernel: one loop, watcher, flusher and idler, of the new session; requests are served', async () => {
    assert(afterSnapshot, 'no image from K30');
    const X = await kernel({ files: kernelFiles(afterSnapshot.image, afterSnapshot.changes) });
    try {
      const h = await X.hello();
      assert(X.events()[0] === h, 'hello first: ' + X.tail());
      // (what the processes of the saved session write, before the kernel
      // ends them)
      const before = X.items.slice(0, X.items.findIndex(i => i.ev === h)).filter(i => i.fd);
      if (before.length) console.log('#   before hello: ' + J(before.map(i => i.text)).slice(0, 300));
      await ok(X, 'Smalltalk at: #NbSnapMarker', ['30']);
      await ok(X, "#('NotebookKernel loop' 'NotebookKernel watcher' 'NotebookKernel flusher' 'NotebookKernel idler') collect: [ :n | " +
                  '(Process allInstances select: [ :p | p isTerminated not and: [ p name = n ] ]) size ]', ['#(1 1 1 1)']);
      for (let i = 0; i < 3; i++) await ok(X, `${i} * 2`, [String(i * 2)]);
      assert(!X.events().some(e => e.ev === 'done' && !X.find(s => s.ev === 'start' && s.rid === e.rid)), 'a done of no cell');
    } finally { await X.close(); }
  });

  await check('K26 a cell breaks what the kernel uses (the Transcript global, SmallInteger>>printOn:): the events stay valid', async () => {
    const X = await kernel();
    try {
      await X.hello();
      await ok(X, 'Smalltalk at: #Transcript put: nil. 1', ['1']);
      const t = await ok(X, "Transcript show: 'still'; flush. 2", ['2']);
      eq(t.out, 'still', 'the Transcript is back');
      await ok(X, "k26old := SmallInteger compiledMethodAt: #printOn: ifAbsent: [ nil ]. " +
                  "SmallInteger compile: 'printOn: s s nextPutAll: ''broken''' classified: 'nbharness'. 0");
      try {
        const r = await ok(X, '7');
        assert(r.done.rid === X.rid && Number.isInteger(r.done.ms) && r.done.values.length === 1, 'done ' + J(r.done));
        const w = await ok(X, "Transcript show: 12 printString; flush. 'w'", ["'w'"]);
        assert(w.out.length > 0, 'the Transcript ' + J(w.out));
        const e = await failing(X, 'nil foo');
        assert(Number.isInteger(e.done.error.line) && e.done.rid === X.rid, 'error ' + J(e.done));
      } finally {
        await ok(X, 'k26old ifNil: [ SmallInteger removeSelector: #printOn: ] ifNotNil: [ :m | SmallInteger addSelector: #printOn: withMethod: m ]. 3', ['3']);
      }
      await ok(X, '3 + 4', ['7']);
      // K27': a cell that takes the standard streams away
      for (const streams of ['useNullStreams', 'useMemoryStreams']) {
        await ok(X, `Stdio ${streams}. 1`, ['1']);
        const r = await ok(X, "Transcript show: 't'; flush. Stdio stdout nextPutAll: 's'; flush. 2", ['2']);
        eq(r.out, 'ts', 'the output after Stdio ' + streams);
      }
    } finally { await X.close(); }
  });
  return true;
}

// ---- Part F: what needs a feature of the build

async function partF() {
  if (!manifest.ffi) {
    skip('F1, F1b, F2, F3', 'manifest.ffi is ' + J(manifest.ffi));
    await check('F0 an FFI cell without the FFI is an error, not a crash', async () => {
      const r = await K.run(defineClass('NbNoFFI') + ". NbNoFFI compile: 'strlen: s ^ self ffiCall: #(size_t strlen(String s)) library: LibC'. " +
                            "NbNoFFI new strlen: 'abc'");
      assert(r.done.status === 'error', 'status ' + r.done.status);
      await ok(K, '3 + 4', ['7']);
    });
    return;
  }
  await check('F1 a strlen callout from a cell', async () => {
    await ok(K, defineClass('NbFFI') + ". NbFFI compile: 'strlen: s ^ self ffiCall: #(size_t strlen(String s)) library: LibC'. " +
                "NbFFI compile: 'qs: b n: n s: s with: c ^ self ffiCall: #(void qsort(ByteArray b, size_t n, size_t s, FFICallback c)) library: LibC'. 0", ['0']);
    await ok(K, "NbFFI new strlen: 'hello'", ['5']);
  });
  const qsort = '| a cb | a := ByteArray new: 24. #(5 3 9 1 7 2) doWithIndex: [ :e :i | a at: i * 4 - 3 put: e ]. ' +
                'cb := FFICallback signature: #(int (void *x, void *y)) block: [ :x :y | (x signedLongAt: 1) - (y signedLongAt: 1) ]. ' +
                'NbFFI new qs: a n: 6 s: 4 with: cb. (1 to: 6) collect: [ :i | a at: i * 4 - 3 ]';
  await check('F1b a qsort callback, then stop-background, then the callback again', async () => {
    await ok(K, qsort, ['#(1 2 3 5 7 9)']);
    const n = K.events().length;
    K.request({ op: 'stop-background' });
    const stopped = await waitFor('stopped', () => { K.alive(); return K.events().slice(n).find(x => x.ev === 'stopped'); });
    console.log(`#   stop-background stopped ${stopped.count} processes`);
    await ok(K, qsort, ['#(1 2 3 5 7 9)']);
  });
  const opaque = d => {
    let n = 0;
    for (let i = 3; i < d.attachment.length; i += 4) if (d.attachment[i] === 255) n++;
    return n;
  };
  if (!(manifest.libraries || []).includes('libcairo.so.2')) skip('F2', 'no libcairo.so.2 in manifest.libraries');
  else await check('F2 an AthensCairoSurface and an RSCanvas are displayed, with opaque pixels', async () => {
    let r = await ok(K, '| s | s := AthensCairoSurface extent: 20 @ 10. s drawDuring: [ :c | c setPaint: Color red. ' +
                        'c drawShape: (0 @ 0 extent: 20 @ 10) ]. s', null, 60000);
    let d = r.displays[0];
    assert(d && d.mime === 'image/x-pharo-bgra' && d.width === 20 && d.height === 10 && d.premultiplied === true,
           'the surface ' + J(Object.assign({}, d, { attachment: undefined })));
    assert(opaque(d) === 200, opaque(d) + ' opaque pixels of 200');
    r = await ok(K, '| c | c := RSCanvas new. c add: (RSBox new size: 40; color: Color blue; yourself). c', null, 60000);
    d = r.displays[0];
    assert(d && d.mime === 'image/x-pharo-bgra' && opaque(d) > 0, 'the canvas ' + J(Object.assign({}, d, { attachment: undefined })));
    console.log(`#   the canvas: ${d.width}x${d.height}, ${opaque(d)} opaque pixels, premultiplied ${d.premultiplied}`);
  });
  if (manifest.fonts !== 'freetype') skip('F3', 'manifest.fonts is ' + J(manifest.fonts));
  else await check('F3 text drawn into a Form with FreeType', async () => {
    const r = await ok(K, '| f | f := Form extent: 100 @ 20 depth: 32. f fillWhite. f getCanvas drawString: \'Hello\' at: 2 @ 2 ' +
                          'font: StandardFonts defaultFont color: Color black. f', null, 60000);
    const d = r.displays[0];
    let dark = 0;
    for (let i = 0; i < d.attachment.length; i += 4) if (d.attachment[i] < 128) dark++;
    assert(d.width === 100 && dark > 20, dark + ' dark pixels');
  });
}

// K31: the end of the input ends the kernel, with 0
async function eofCases() {
  await check('K31 the end of stdin: the kernel exits 0', async () => {
    await K.idle();
    K.drv.eof();
    await waitFor('exit', () => K.exit !== null || K.crash, 30000);
    eq(K.exit, 0, 'the exit code');
    assert(K.drv.dead && K.drv.state() === EXITED, 'the driver is dead');
  });
  const host = process.env.HOST_PHARO;
  if (!host || !fs.existsSync(host)) { skip('K31 native', 'no HOST_PHARO'); return; }
  await check('K31 native: NOTEBOOK_EVENTS=/dev/fd/3, a cell, then the end of stdin: exit 0', async () => {
    const dir = fs.mkdtempSync(path.join(scratch, 'native-'));
    fs.writeFileSync(path.join(dir, 'Pharo.image'), stock.image);
    fs.writeFileSync(path.join(dir, 'Pharo.changes'), stock.changes);
    fs.symlinkSync(path.join(imageDir, stock.sourcesName), path.join(dir, stock.sourcesName));
    const evs = [], errors = [];
    const reader = new Kernel.FrameReader(ev => evs.push(ev), text => errors.push(text));
    const p = childProcess.spawn(host, ['--headless', path.join(dir, 'Pharo.image'), '--no-default-preferences', 'st',
                                        '--no-source', path.join(stDir, 'web-notebook.st')],
                                 { cwd: dir, env: Object.assign({}, process.env, { NOTEBOOK_EVENTS: '/dev/fd/3' }),
                                   stdio: ['pipe', 'pipe', 'pipe', 'pipe'] });
    let out = '', err = '';
    p.stdout.on('data', d => { out += d; });
    p.stderr.on('data', d => { err += d; });
    p.stdio[3].on('data', d => reader.push(new Uint8Array(d)));
    const exited = new Promise(r => p.on('exit', code => r(code)));
    p.stdin.write(J({ op: 'run', rid: 1, name: 'In[1]', code: "Transcript show: 'n'; flush. 3 + 4" }) + '\n');
    await waitFor('done', () => evs.find(e => e.ev === 'done'), 60000);
    p.stdin.end();
    const code = await Promise.race([exited, sleep(30000).then(() => 'still running')]);
    if (code === 'still running') p.kill('SIGKILL');
    eq(code, 0, 'the exit status');
    eq(evs.map(e => e.ev), ['hello', 'start', 'done'], 'the events');
    eq(evs[2].values, ['7'], 'values');
    eq(errors, [], 'protocol errors');
    assert(out === 'n', 'the output ' + J(out) + ', stderr ' + J(err.slice(0, 300)));
    fs.rmSync(dir, { recursive: true, force: true });
  });
}


// ---- Part C: nb-kernel.js against the real vm-worker.js

const memory = PharoStorage.memory();
let shim = null, workers = 0;
const allWorkers = [];
// A worker of the page; `tap' records the type of every message it posts
function newWorker(opts = {}) {
  const w = shim.webWorker(opts);
  w.tap = [];
  w.node.on('message', m => w.tap.push(m && m.type));
  workers++;
  allWorkers.push(w);
  return w;
}

function client(extra = {}) {
  const c = { cells: {}, states: [], logs: [], all: [], workers: [] };
  c.k = Kernel.create(Object.assign({
    createWorker: () => { const w = newWorker(c.workerOpts); c.workers.push(w); return w; },
    getModule: () => Promise.resolve(wasmModule),
    init: { manifestUrl: 'manifest.json?v=' + encodeURIComponent(manifest.build), build: manifest.build, persist: true, gitProxy: '' },
    files: () => [],
    schedule: f => setImmediate(f),
    onCell: (id, ev) => { (c.cells[id] = c.cells[id] || []).push(ev); c.all.push([id, ev]); },
    onState: st => { c.states.push(st); c.all.push(['state', st]); },
    onLog: (kind, text) => c.logs.push([kind, text]),
  }, extra));
  c.types = id => (c.cells[id] || []).map(e => e.type);
  c.stream = (id, name) => (c.cells[id] || []).filter(e => e.type === 'stream' && (!name || e.name === name)).map(e => e.text).join('');
  c.tail = () => J({ states: c.states.slice(-6), logs: c.logs.slice(-4),
                     all: c.all.slice(-4).map(([id, ev]) => [id, typeof ev === 'object' ? Object.assign({}, ev, { data: ev.data && ev.data.slice(0, 80) }) : ev]) })
    .slice(-1500);
  c.worker = () => c.workers[c.workers.length - 1];
  current = c;
  return c;
}
const fields = d => [d.status || d.reason, d.values];

// A string to search, and the key not in it: the search runs for about
// `ms' in one primitive (MiscPrimitivePlugin's primitiveFindSubstring),
// which neither the watcher nor the worker can interrupt.  Calibrated on
// this engine, from 1 MB.
let longSearch = null;
async function calibrate(C) {
  const d = await C.k.run('cal', "| s k | s := String new: 1000000 withAll: $a. k := (String new: 300 withAll: $a) , 'b'. " +
                                 '[ s findString: k startingAt: 1 ] timeToRun asMilliSeconds');
  const t = Math.max(1, Number(d.values && d.values[0]));
  const size = Math.min(64e6, Math.ceil(1e6 * 6000 / t));
  longSearch = { size, ms: t * size / 1e6,
                 setup: `nbS := String new: ${size} withAll: $a. nbK := (String new: 300 withAll: $a) , 'b'. 0`,
                 run: 'nbS findString: nbK startingAt: 1' };
  console.log(`#   1 MB searched in ${t} ms: ${size} bytes for about ${longSearch.ms.toFixed(0)} ms`);
}

async function partC() {
  shim = await WorkerShim.start(webDir, { memory });
  // an image of another Pharo is the Console's slot
  const seed = async () => {
    memory.map.clear();
    if (!otherPharo) return;
    await PharoStorage.open(memory).save(stock.image, stock.changes, { build: manifest.build, prepared: false, webPackage: 0 },
                                         { name: stock.sourcesName, data: stock.sources }, null);
  };
  await seed();
  if (otherPharo) console.log('# Part C boots ' + imageName + ' from the slot');
  let C;

  await check('C1 start() resolves on hello: off, starting, idle; info {proto 1, major}', async () => {
    C = home = client();
    eq(C.k.state, 'off', 'off at first');
    await C.k.start();
    eq(C.states, ['starting', 'idle'], 'states');
    const i = C.k.info;
    assert(i && i.proto === 1 && i.major === major && /^1[25]\.\d+$/.test(i.version) && i.wordSize === 8 &&
           i.source === (otherPharo ? 'saved' : 'download'), 'info ' + J(i));
  });
  if (!C || C.k.state !== 'idle') { console.log('cannot continue'); return; }

  await check('C2 run gives queued, start, done; the promise resolves with done', async () => {
    const d = await C.k.run('a', '1 + 2');
    eq(C.types('a'), ['queued', 'start', 'done'], 'events');
    eq(C.cells.a[1], { type: 'start', count: 1, name: 'In[1]' }, 'start');
    assert(d === C.cells.a[2] && d.status === 'ok', 'resolved with done');
    eq([d.values, d.count], [['3'], 1], 'values and count');
    assert(Number.isInteger(d.wallMs) && Number.isInteger(d.ms), 'times ' + J(d));
    eq(C.k.state, 'idle', 'idle');
  });

  await check('C3 runMany stops at an error: previous cell failed; terminated is not a failure', async () => {
    const r = await C.k.runMany([{ cellId: 'm1', source: "Transcript show: 'one'; cr. 1" },
                                 { cellId: 'm2', source: 'nil foo' }, { cellId: 'm3', source: '3' }]);
    eq(r.map(e => e.status || e.reason), ['ok', 'error', 'previous cell failed'], 'outcomes');
    eq(C.stream('m1'), 'one\n', 'stream');
    eq(C.types('m3'), ['queued', 'cancelled'], 'm3 events');
    eq(C.k.count, 3, 'the counter');
    const t = await C.k.runMany([{ cellId: 'm4', source: 'Processor activeProcess terminate' }, { cellId: 'm5', source: '1 + 2' }]);
    eq(t.map(e => e.status || e.reason), ['terminated', 'ok'], 'terminated outcomes');
  });

  await check('C4 stop() interrupts the running cell and cancels the queue; the next cell works', async () => {
    const p = C.k.run('loop', '[ true ] whileTrue'), q = C.k.run('after', '1');
    await waitFor('busy', () => C.k.current === 'loop' && C.k.state === 'busy');
    await sleep(200);
    C.k.stop();
    eq((await p).status, 'interrupted', 'interrupted');
    eq((await q).reason, 'stopped', 'the queue cancelled');
    eq((await C.k.run('next', '6 * 7')).values, ['42'], 'the next cell');
  });

  await calibrate(C).catch(e => console.log('# calibration failed: ' + e.message));

  await check('C5 the watchdog: a Stop unanswered for 1000 ms kills and respawns the kernel', async () => {
    assert(longSearch, 'not calibrated');
    const W = client({ watchdogMs: 1000 });
    try {
      await W.k.run('d', 'k5small := 1');
      eq((await W.k.run('s', longSearch.setup)).status, 'ok', 'the setup');
      const before = workers;
      const p = W.k.run('big', longSearch.run);
      await waitFor('busy', () => W.k.current === 'big' && W.k.state === 'busy');
      await sleep(500);                 // in the search
      const t = now();
      W.k.stop();
      const d = await within(p, 20000, 'the end of the cell');
      const ms = now() - t;
      eq([d.status, d.reason], ['killed', 'unresponsive'], 'killed');
      assert(ms >= 998 && ms < 5000, `killed after ${ms.toFixed(0)} ms`);
      assert(W.logs.some(([k, x]) => k === 'kernel' && /did not respond/.test(x)), 'the log ' + J(W.logs));
      const r = await W.k.run('after', 'k5small');
      assert(r.status === 'error' && /k5small/.test(r.error.text), 'k5small is gone: ' + J(r));
      eq(r.count, 1, 'the counter restarted');
      eq(workers, before + 1, 'workers spawned');
      console.log(`#   killed after ${ms.toFixed(0)} ms`);
    } finally { W.k.dispose() }
  });

  await check('C5b a Stop that the watcher cannot deliver: killed within [3000, 10500) ms, once, whatever the ticks', async () => {
    const W = client();
    try {
      await W.k.start();
      for (const [code, cap] of [['[ [ true ] whileTrue ] valueUnpreemptively', false],
                                 ['[ [ true ] whileTrue ] forkAt: Processor userInterruptPriority + 1. Semaphore new wait', false],
                                 ['[ [ true ] whileTrue: [ | t | t := Time millisecondClockValue + 100. ' +
                                  "[ Time millisecondClockValue < t ] whileTrue. Stdio stdout nextPutAll: '.'; flush ] ] valueUnpreemptively", true]]) {
        const before = workers, w = W.worker();
        const p = W.k.run('h', code);
        await waitFor('busy', () => W.k.current === 'h' && W.k.state === 'busy');
        await sleep(300);
        const t = now(), ticks = w.tap.filter(x => x === 'tick').length;
        W.k.stop();
        const d = await within(p, 20000, 'the end of the cell');
        const ms = now() - t;
        eq([d.status, d.reason], ['killed', 'unresponsive'], code.slice(0, 40));
        assert(ms >= (cap ? 9500 : 3000) && ms < 10500, `${code.slice(0, 40)}: killed after ${ms.toFixed(0)} ms`);
        // (a cell that prints keeps the worker posting output, and ticks
        // come only after a second without any)
        if (!cap) assert(w.tap.filter(x => x === 'tick').length > ticks, 'ticks while stopping');
        else assert(W.stream('h').length >= 20, 'output while stopping: ' + W.stream('h').length);
        await waitFor('idle', () => W.k.state === 'idle', 60000);
        eq(workers, before + 1, 'one respawn');
        console.log(`#   ${code.slice(0, 40)}...: killed after ${ms.toFixed(0)} ms`);
      }
    } finally { W.k.dispose() }
  });

  await check('C6 restart() kills the running cell; the counter restarts', async () => {
    const p = C.k.run('r1', '[ true ] whileTrue'), q = C.k.run('r2', '1');
    await waitFor('busy', () => C.k.current === 'r1');
    await C.k.restart();
    eq([(await p).status, (await p).reason], ['killed', 'restart'], 'killed');
    eq((await q).reason, 'restart', 'the queue cancelled');
    eq(C.k.count, 0, 'the counter');
    const d = await C.k.run('r3', 'nb6 := 1. nb6');
    eq([d.count, d.values], [1, ['1']], 'a fresh kernel');
  });

  await check('C7 a cell that waits for stdin: input {waiting}, state input; Stop ends it', async () => {
    const p = C.k.run('in', 'Stdio stdin waitForData. 1');
    await waitFor('input', () => (C.cells.in || []).some(e => e.type === 'input' && e.waiting));
    eq(C.k.state, 'input', 'state');
    C.k.stop();
    eq((await p).status, 'interrupted', 'status');
    eq(C.cells.in.filter(e => e.type === 'input').map(e => e.waiting), [true, false], 'waiting, then not');
    eq((await C.k.run('in2', '1 + 1')).values, ['2'], 'the next cell');
  });

  await check('C8 Smalltalk exit: 2: done exited, state dead; the next run respawns', async () => {
    const d = await C.k.run('ex', 'Smalltalk exit: 2');
    eq([d.status, d.code], ['exited', 2], 'exited');
    eq(C.k.state, 'dead', 'dead');
    eq((await C.k.run('again', '1 + 2')).values, ['3'], 'respawned');
    assert(C.logs.some(([k, t]) => k === 'kernel' && t === 'kernel restarted'), 'logged ' + J(C.logs));
  });

  await check('C9 a Transcript flood of 200000 lines with batched acks: complete, in order, done last', async () => {
    const d = await C.k.run('flood', '1 to: 200000 do: [ :i | Transcript show: i printString; cr ]');
    eq(d.status, 'ok', 'status');
    let expect = '';
    for (let i = 1; i <= 200000; i++) expect += i + '\n';
    const s = C.stream('flood');
    assert(s === expect, `all lines in order (${s.length} of ${expect.length})`);
    eq(C.types('flood').slice(-1), ['done'], 'done last');
  });

  await check('C9b acks scheduled with setTimeout (a hidden page): 5 MB arrives, the VM never stalls on credit', async () => {
    const H = client({ schedule: f => setTimeout(f, 0) });
    try {
      const t = now();
      const d = await H.k.run('big', '1 to: 50000 do: [ :i | Stdio stdout nextPutAll: (String new: 99 withAll: $x); lf ]');
      eq(d.status, 'ok', 'status');
      assert(H.stream('big') === ('x'.repeat(99) + '\n').repeat(50000), 'the output: ' + H.stream('big').length);
      console.log(`#   5 MB in ${(now() - t).toFixed(0)} ms`);
    } finally { H.k.dispose() }
  });

  await check('C11 the order of the events; cancel() removes a queued cell; sleeping is seen', async () => {
    const p = C.k.run('slow', "Stdio stdout nextPutAll: 'o'; flush. Stdio stderr nextPutAll: 'e'; flush. " +
                              "Notebook show: (Notebook html: '<i>h</i>') id: 'id1'. Notebook clearOutput. (Delay forSeconds: 1) wait. 5");
    const q = C.k.run('gone', '1');
    C.k.cancel('gone');
    eq((await q).reason, 'removed', 'removed');
    const d = await p;
    eq(d.values, ['5'], 'value');
    eq(C.types('slow'), ['queued', 'start', 'stream', 'stream', 'display', 'clear', 'done'], 'events');
    eq(C.cells.slow[4], { type: 'display', mime: 'text/html', data: '<i>h</i>', id: 'id1' }, 'display');
    eq(C.cells.slow.slice(2, 4).map(e => e.name + ':' + e.text), ['stdout:o', 'stderr:e'], 'streams');
    assert(C.states.includes('sleeping'), 'sleeping seen');
  });

  await check('C11b a display of 5 MB comes with its size only', async () => {
    const d = await C.k.run('svg', "Notebook svg: '<svg>' , (String new: 5000000 withAll: $a) , '</svg>'");
    eq(d.status, 'ok', 'status');
    const shown = C.cells.svg.filter(e => e.type === 'display');
    assert(shown.length === 1 && shown[0].data === '' && shown[0].size > 4194304 && shown[0].mime === 'image/svg+xml',
           'display ' + J(shown.map(e => Object.assign({}, e, { data: e.data.slice(0, 20) }))));
  });

  await check('C12 uploads are written again after a respawn; writeFile; dispose', async () => {
    const U = client({ files: () => [{ path: '/pharo/up.st', data: utf8('Smalltalk at: #NbUp put: 2!\n') }] });
    try {
      eq((await U.k.run('u', "'up.st' asFileReference fileIn. Smalltalk at: #NbUp")).values, ['2'], 'the upload');
      await U.k.restart();
      eq((await U.k.run('u2', "'up.st' asFileReference fileIn. Smalltalk at: #NbUp")).values, ['2'], 'after the restart');
      eq(await U.k.writeFile('/pharo/w.st', utf8('Smalltalk at: #NbW put: 3!\n')), null, 'writeFile');
      eq((await U.k.run('w', "'w.st' asFileReference fileIn. Smalltalk at: #NbW")).values, ['3'], 'the file written');
    } finally { U.k.dispose(); }
    eq((await U.k.run('x', '1')).reason, 'disposed', 'disposed');
  });

  await check('C13 getModule rejects: start() rejects with the reason; unavailable', async () => {
    const B = client({ getModule: () => Promise.reject(new Error('no module here')) });
    let why = null;
    await B.k.start().catch(e => { why = e; });
    assert(why && /no module here/.test(why.reason), 'rejected: ' + J(why));
    eq(B.k.state, 'unavailable', 'state');
    eq((await B.k.run('x', '1')).reason, 'kernel unavailable', 'run cancelled');
    B.k.dispose();
  });

  await check('C14 restart() while starting: both promises resolve on the new hello', async () => {
    const R = client();
    try {
      const p1 = R.k.start(), p2 = R.k.restart();
      await Promise.all([p1, p2]);
      eq(R.k.state, 'idle', 'idle');
      eq((await R.k.run('x', '1 + 1')).values, ['2'], 'runs');
    } finally { R.k.dispose() }
  });

  await check('C15 a kernel that exits before hello: start() rejects, its stderr is logged', async () => {
    const F = client();
    F.workerOpts = { override: { '/st/web-notebook.st': "Stdio stderr nextPutAll: 'nb-harness: no kernel here'; lf; flush. Smalltalk exit: 4!\n" } };
    try {
      let why = null;
      await F.k.start().catch(e => { why = e; });
      assert(why && /exited/.test(why.reason), 'rejected: ' + J(why));
      eq(F.k.state, 'dead', 'dead');
      assert(F.logs.some(([k, t]) => k === 'stderr' && /no kernel here/.test(t)), 'stderr logged ' + J(F.logs));
      eq((await F.k.run('x', '1')).reason, 'kernel exited', 'run cancelled');
    } finally { F.k.dispose() }
  });

  await check('C16 C17 the watchdog after a Delay and after a read of stdin, ending in the long primitive', async () => {
    assert(longSearch, 'not calibrated');
    const W = client({ watchdogMs: 1000 });
    try {
      for (const [what, first, seen] of [['a Delay', '(Delay forMilliseconds: 700) wait', 'sleeping'],
                                         ['stdin', '| t | Stdio stdin next: 1. t := Time millisecondClockValue + 700. ' +
                                                   '[ Time millisecondClockValue < t ] whileTrue: [ (Delay forMilliseconds: 10) wait ]', 'input']]) {
        eq((await W.k.run('s', longSearch.setup)).status, 'ok', 'the setup');
        const from = W.states.length;
        const p = W.k.run('big', first + '. ' + longSearch.run);
        await waitFor(seen, () => W.states.slice(from).includes(seen), 10000);
        // the search has begun, in the slice that ended the wait (which
        // posts no state until it is over)
        await sleep(1200);
        const t = now();
        W.k.stop();
        const d = await within(p, 20000, 'the end of the cell');
        const ms = now() - t;
        eq([d.status, d.reason], ['killed', 'unresponsive'], what);
        assert(ms >= 998 && ms < 5000, `${what}: killed after ${ms.toFixed(0)} ms`);
        await waitFor('idle', () => W.k.state === 'idle', 60000);
      }
    } finally { W.k.dispose() }
  });

  await check('C20 a state of the slice before a cell started never reports the cell idle', async () => {
    const R = await C.k.runMany(Array.from({ length: 30 }, (_, i) => ({ cellId: 'q' + i, source: String(i) })));
    eq(R.map(d => d.values[0]), Array.from({ length: 30 }, (_, i) => String(i)), 'values');
    for (let i = 0; i < 30; i++) {
      const a = C.all.findIndex(([id, ev]) => id === 'q' + i && ev.type === 'start');
      const b = C.all.findIndex(([id, ev]) => id === 'q' + i && ev.type === 'done');
      assert(!C.all.slice(a + 1, b).some(([id, st]) => id === 'state' && st === 'idle'), 'idle during q' + i);
    }
  });

  await check('C23 an image decodes to its pixels, 50% alpha too; forged headers are protocol errors, and the cell goes on', async () => {
    const d = await C.k.run('img', '| f | f := Form extent: 2 @ 1 depth: 32. f colorAt: 0 @ 0 put: Color red. ' +
                                   'f colorAt: 1 @ 0 put: (Color red alpha: 0.5). f');
    eq(d.status, 'ok', 'status');
    const shown = C.cells.img.filter(e => e.type === 'display');
    assert(shown.length === 1 && shown[0].mime === 'image/png' && shown[0].width === 2 && shown[0].height === 1, 'display ' + J(shown));
    const png = decodePng(Buffer.from(shown[0].data, 'base64'));
    assert(near(pixel(png, 0), [255, 0, 0, 255]) && near(pixel(png, 1), [255, 0, 0, 128]), 'pixels ' + J([pixel(png, 0), pixel(png, 1)]));
    // a cell writes events of its own on the kernel's stream
    const forge = (header, n) => '| ev | ev := NotebookKernel classPool at: #Events. ' +
      `ev nextPutAll: ('${J(header)}' , (String with: Character lf)) asByteArray; nextPutAll: (ByteArray new: ${n}); ` +
      'nextPutAll: #[10]; flush. 1';
    for (const [what, header, n] of [
      ['width 1e6', { ev: 'display', rid: 0, mime: 'image/x-pharo-bgra', width: 1000000, height: 1, premultiplied: false, id: null, bytes: 4 }, 4],
      ['bytes != 4wh', { ev: 'display', rid: 0, mime: 'image/x-pharo-bgra', width: 2, height: 2, premultiplied: false, id: null, bytes: 15 }, 15],
      ['over the cap', { ev: 'display', rid: 0, mime: 'image/png', id: null, bytes: Kernel.MAX_ATTACHMENT + 1 }, Kernel.MAX_ATTACHMENT + 1]]) {
      const logs = C.logs.length;
      const id = 'forged ' + what;
      header.rid = C.k.count + 1;
      const r = await C.k.run(id, forge(header, n));
      eq(fields(r), ['ok', ['1']], what);
      assert(C.logs.slice(logs).some(([k]) => k === 'protocol'), what + ': no protocol error: ' + J(C.logs.slice(logs)));
      assert(!C.cells[id].some(e => e.type === 'display'), what + ': displayed');
    }
    eq((await C.k.run('after', '3 + 4')).values, ['7'], 'the kernel goes on');
  });

  await check('C23b a done that a cell forges for its own rid ends it, and the cells queued after it get their own results', async () => {
    const logs = C.logs.length;
    const forged = { ev: 'done', rid: C.k.count + 1, ms: 0, status: 'ok', values: ['FORGED'] };
    const R = await C.k.runMany([
      { cellId: 'fd-a', source: '| ev | ev := NotebookKernel classPool at: #Events. ' +
        `ev nextPutAll: ('${J(forged)}' , (String with: Character lf)) asByteArray; flush. 'real-a'` },
      { cellId: 'fd-b', source: "'cell-b'" }, { cellId: 'fd-c', source: "'cell-c'" }]);
    eq(R.map(d => d.values), [['FORGED'], ["'cell-b'"], ["'cell-c'"]], 'the values');
    assert(C.logs.slice(logs).some(([k, t]) => k === 'protocol' && /^done for rid/.test(t)), 'no protocol error: ' + J(C.logs.slice(logs)));
  });

  C.k.dispose();
  home = null;

  // the slot of the Console: written by a Console worker (the negative
  // control of C21), then booted read-only by the kernel
  let consoleSaved = false;
  await check('C21 negative control: a Console worker stores its save (PageMarker 777)', async () => {
    memory.map.clear();
    if (otherPharo) await seed();
    const w = newWorker();
    try {
      const msgs = [];
      let out = '';
      w.onmessage = e => { msgs.push(e.data); if (e.data.type === 'output' && e.data.fd !== 3) { out += e.data.text; w.postMessage({ type: 'ack', chars: e.data.text.length }); } };
      w.postMessage({ type: 'init', wasmModule, manifestUrl: 'manifest.json?v=' + encodeURIComponent(manifest.build),
                      build: manifest.build, mode: 'console', persist: true });
      await waitFor('the prompt', () => /st> $/.test(out), 60000);
      w.postMessage({ type: 'input', text: 'Smalltalk at: #PageMarker put: 777\n' });
      await waitFor('777', () => /777\nst> $/.test(out), 30000);
      w.postMessage({ type: 'save' });
      const saved = await waitFor('saved', () => msgs.find(m => m.type === 'saved'), 120000);
      assert(!saved.error && msgs.some(m => m.type === 'storing'), 'saved ' + J(saved));
      assert(memory.map.get('meta') && memory.map.get('meta').imageSize === saved.bytes, 'the slot');
      consoleSaved = true;
    } finally { w.terminate(); }
  });

  await check('C21 the kernel never stores: no storing or saved, the slot unchanged after a snapshot in a cell and a save', async () => {
    const before = new Map(memory.map), meta = J(memory.map.get('meta') || null);
    const N = client();
    try {
      eq(fields(await N.k.run('snap', 'Smalltalk snapshot: true andQuit: false. 1')), ['ok', ['1']], 'the snapshot');
      N.worker().postMessage({ type: 'save' });
      N.worker().postMessage({ type: 'flush' });
      await sleep(6000);              // more than the worker's SYNC_MS
      eq((await N.k.run('after', '3 + 4')).values, ['7'], 'the kernel goes on');
      const kinds = N.worker().tap;
      for (const k of ['storing', 'saved', 'edited', 'superseded']) assert(!kinds.includes(k), k + ' posted');
      assert(memory.map.size === before.size && [...before].every(([k, v]) => memory.map.get(k) === v) &&
             J(memory.map.get('meta') || null) === meta, 'the slot changed');
    } finally { N.k.dispose(); }
  });

  const imageUrl = '/' + manifest.files.find(f => f.path === manifest.image).url;
  await check('C22 with a slot the kernel boots it without fetching the image; without one, the manifest image', async () => {
    assert(consoleSaved, 'no slot (C21)');
    const N = client();
    try {
      eq((await N.k.run('m', 'Smalltalk at: #PageMarker')).values, ['777'], 'the Console global');
      eq(N.k.info.source, 'saved', 'source');
      assert(!N.worker().site.log.includes(imageUrl), 'the image was fetched: ' + N.worker().site.log.join(' '));
    } finally { N.k.dispose(); }
    memory.map.clear();
    const M = client();
    try {
      eq((await M.k.run('m', "Smalltalk at: #PageMarker ifAbsent: [ #none ]")).values, ['#none'], 'no Console global');
      eq(M.k.info.source, 'download', 'source');
      assert(M.worker().site.log.includes(imageUrl), 'the image was not fetched: ' + M.worker().site.log.join(' '));
    } finally { M.k.dispose(); }
  });
}

(async () => {
  const t0 = now();
  if (await partK()) {
    await partF();
    await eofCases();
  }
  await partC();
  for (const w of allWorkers) w.terminate();
  // the crashes asked for excepted
  kernels.forEach((s, i) => {
    const text = s.output(1) + s.output(2) + s.diag + (s.crash || '');
    const found = !s.crashExpected && text.match(ENGINE_ERRORS);
    if (found) { failures++; console.log(`not ok - engine error in kernel ${i + 1}: ` + found[0]); }
  });
  allWorkers.forEach((w, i) => {
    const found = w.diag.match(ENGINE_ERRORS);
    if (found) { failures++; console.log(`not ok - engine error in worker ${i + 1}: ` + found[0]); }
  });
  fs.rmSync(scratch, { recursive: true, force: true });
  console.log(`# ${passes} passed, ${failures} failed, ${passes + failures} cases in ${((now() - t0) / 1000).toFixed(1)} s`);
  process.exit(failures ? 1 : 0);
})().catch(e => { console.log('not ok - ' + ((e && e.stack) || e)); process.exit(1); });
