// worker-harness.js - run the real vm-worker.js in worker_threads, without a browser
//
// usage: node [v8 flags] worker-harness.js WEB_DIR
//
// WEB_DIR is a built web directory (build-wasm/web), served over HTTP by
// packaging/emscripten/tools/serve.mjs as by `make wasm-serve'.  Each session
// runs WEB_DIR/vm-worker.js, unmodified, in a worker_threads Worker with a
// 1 MB stack (about what browsers give workers), behind a small shim for the
// Web Worker globals it uses: self, location, importScripts (the files of
// WEB_DIR, through vm.runInThisContext), postMessage and onmessage, fetch
// (of the server, relative to the worker's URL), and a fair MessageChannel.
// node has the rest (DecompressionStream, Blob, TextDecoder).  The storage
// of vm-storage.js is an in-memory store of this process (PharoStorage
// .memory), which outlives the workers as IndexedDB outlives a page: the
// shim makes it PharoStorage.backend.
//
// The test plays the page: it sends the messages of vm-worker.js's protocol
// and acks the output it gets.  Every session has a URL prefix of its own,
// so that the server can log its requests and serve it as asked (with
// Content-Encoding: gzip, or other contents for some files).  Besides the
// protocol, Stop, the output credit, the downloads and the persistence, it
// checks what Open gives the worker: an image with a .sources of its own,
// which the slot keeps with it, and the preparation for the world of an
// image without OSWindow-Web (init.prepare).  Prints every case and their
// count, and exits with status 1 if any fails.  Lane 70
// (tests/wasm/lanes/70-worker-harness.sh) runs it.

'use strict';
const { Worker, MessageChannel } = require('worker_threads');
const crypto = require('crypto');
const fs = require('fs');
const http = require('http');
const os = require('os');
const path = require('path');
const { pathToFileURL } = require('url');
const zlib = require('zlib');

const webDir = path.resolve(process.argv[2] || 'build-wasm/web');
const srcDir = path.join(__dirname, '..', '..');
const manifest = JSON.parse(fs.readFileSync(path.join(webDir, 'manifest.json'), 'utf8'));
const wasmModule = new WebAssembly.Module(fs.readFileSync(path.join(webDir, 'pharo-web.wasm')));
const PharoStorage = require(path.join(webDir, 'vm-storage.js'));
const ENGINE_ERRORS = /RuntimeError|RangeError|Aborted|unreachable|signature_mismatch|unsupported syscall/;
const WAITING = 1, BUSY = 2;
const sizeOf = name => manifest.files.find(f => f.path === name).size;
const sha256 = data => crypto.createHash('sha256').update(data).digest('hex');

// What a page has in IndexedDB.  storeFails makes its writes fail, as a full
// disk; storeUnavailable fails everything, as a database that cannot be
// opened (a private window).
const memory = PharoStorage.memory();
let storeFails = null, storeUnavailable = null;

// A stand-in for display-worker.js (M2), which records what the worker does
// with it in "display" messages
const displayStub = path.join(fs.mkdtempSync(path.join(process.env.TEST_DIR || os.tmpdir(), 'worker-harness-')),
                              'display-worker.js');
fs.writeFileSync(displayStub, `
var PharoDisplay = {
  create(init, post) {
    post({ type: 'display', kind: 'created', init });
    return { open() {}, present() {}, setTitle() {}, setCursor() {}, setClipboard() {}, focus() {},
             handle(m, vm) { post({ type: 'display', kind: 'handle', what: m.kind, state: vm.state(), dead: vm.dead }); } };
  },
  onMessage(m, vm) {
    vm.kick();
    self.postMessage({ type: 'display', kind: 'handled', what: m.what, state: vm.state() });
  },
};
`);

// (In a block: vm-worker.js shares the global scope of this script.)
const prelude = `
  'use strict';
  {
  const { parentPort, workerData } = require('worker_threads');
  const fs = require('fs'), path = require('path'), vm = require('vm');
  const { dir, url, overrides, storePort } = workerData;
  globalThis.self = globalThis;
  self.location = new URL(url);
  globalThis.require = require;               // emscripten's node support
  delete globalThis.module;                   // eval workers define these:
  delete globalThis.exports;                  // UMD would pick CommonJS
  globalThis.__dirname = dir;
  globalThis.onmessage = null;
  const nodeFetch = globalThis.fetch;
  globalThis.fetch = (u, o) => nodeFetch(new URL(u, self.location.href), o);
  // PharoStorage.backend: the store of the harness, through a port
  const calls = new Map();
  let lastCall = 0;
  storePort.on('message', ({ n, value, error }) => {
    const c = calls.get(n);
    calls.delete(n);
    if (error) c.reject(Object.assign(new Error(error.message), error)); else c.resolve(value);
  });
  const call = (op, args) => new Promise((resolve, reject) => {
    calls.set(++lastCall, { resolve, reject });
    storePort.postMessage({ n: lastCall, op, args });
  });
  const store = { get: key => call('get', [key]), write: (e, guard) => call('write', [e, guard]),
                  clear: () => call('clear', []) };
  globalThis.importScripts = (...files) => files.forEach(p => {
    const name = p.split('?')[0], file = overrides[name] || path.join(dir, name);
    vm.runInThisContext(fs.readFileSync(file, 'utf8'), { filename: file });
    if (name === 'vm-storage.js') PharoStorage.backend = store;
  });
  globalThis.postMessage = (m, transfer) => parentPort.postMessage(m, transfer);
  // Node's MessagePort drains up to 1000 queued messages per wakeup, and
  // the worker's pump re-posts one per slice: a busy VM would starve the
  // parentPort (no Stop for a long time).  Browsers queue both as ordinary
  // tasks and interleave them, so give the worker a fair zero-delay
  // channel built on setImmediate, which lets the poll phase run.
  globalThis.MessageChannel = class {
    constructor() {
      const port1 = { onmessage: null };
      this.port1 = port1;
      this.port2 = { postMessage: d => setImmediate(() => port1.onmessage && port1.onmessage({ data: d })) };
    }
  };
  parentPort.on('message', d => onmessage({ data: d }));
  importScripts('vm-worker.js');
  }
`;

// ---- the server: WEB_DIR under /s<session>/, each with its own log and mode

const servers = new Map();              // session number -> {log, encodeGzip, override}
let base = null, serveHandler = null;
function startServer() {
  return import(pathToFileURL(path.join(srcDir, 'packaging', 'emscripten', 'tools', 'serve.mjs'))).then(serve => {
    serveHandler = serve.handler(webDir);
    const server = http.createServer((req, res) => {
      const m = /^\/s(\d+)(\/.*)$/.exec(req.url);
      const s = m && servers.get(Number(m[1]));
      if (!s) { res.writeHead(404); res.end(); return; }
      const rel = m[2].replace(/\?.*/, '');
      s.log.push(rel);
      // other contents for this file
      if (s.override && rel in s.override) {
        res.writeHead(200, { 'Content-Type': 'application/octet-stream' });
        res.end(s.override[rel]);
        return;
      }
      // as a server that sends .gz files with Content-Encoding: gzip, which
      // the client inflates on the way
      if (s.encodeGzip && rel.endsWith('.gz')) {
        res.writeHead(200, { 'Content-Type': 'application/octet-stream', 'Content-Encoding': 'gzip' });
        fs.createReadStream(path.join(webDir, rel)).pipe(res);
        return;
      }
      req.url = m[2];
      serveHandler(req, res);
    });
    return new Promise(r => server.listen(0, '127.0.0.1', () => {
      base = 'http://127.0.0.1:' + server.address().port + '/';
      server.unref();
      r();
    }));
  });
}

// ---- sessions

let failures = 0, passes = 0, sessionCount = 0;
const now = () => performance.now();
const sleep = ms => new Promise(r => setTimeout(r, ms));
function assert(c, msg) { if (!c) throw new Error('assertion failed: ' + msg); }

async function waitFor(what, pred, ms = 30000) {
  const t0 = now();
  for (;;) {
    const v = pred();
    if (v) return v;
    if (now() - t0 > ms) throw new Error('timed out after ' + ms + ' ms waiting for ' + what);
    await sleep(5);
  }
}

let current = null;
const sessions = [];

// One worker.  `autoAck' acks every output message, as the page does after
// rendering it.
function session(init = {}, { autoAck = true, encodeGzip = false, display = false, override = null } = {}) {
  const n = ++sessionCount;
  const s = { n, msgs: [], out: '', err: '', states: [], progress: [], ready: null, exit: null, crash: null,
              saved: [], prepared: null, superseded: 0, files: [], ticks: 0, error: null, diag: '', log: [],
              markAt: 0, errAt: 0, stateAt: 0, msgAt: 0, acked: 0 };
  servers.set(n, { log: s.log, encodeGzip, override });
  const store = new MessageChannel();
  store.port1.on('message', async ({ n: id, op, args }) => {
    try {
      if (storeUnavailable) throw storeUnavailable;
      if (op === 'write' && storeFails) throw storeFails;
      store.port1.postMessage({ n: id, value: await memory[op](...args) });
    } catch (e) {
      store.port1.postMessage({ n: id, error: { name: e.name, message: e.message, unavailable: e.unavailable } });
    }
  });
  s.w = new Worker(prelude, {
    eval: true, stdout: true, stderr: true,
    workerData: { dir: webDir, url: base + 's' + n + '/vm-worker.js?v=' + encodeURIComponent(manifest.build),
                  overrides: display ? { 'display-worker.js': displayStub } : {}, storePort: store.port2 },
    transferList: [store.port2],
    resourceLimits: { stackSizeMb: 1 },
  });
  // what the worker prints (console.warn of the emscripten runtime)
  s.w.stdout.on('data', d => { s.diag += d; });
  s.w.stderr.on('data', d => { s.diag += d; });
  s.w.on('error', e => { s.error = e; });
  s.w.on('message', m => {
    s.msgs.push(m);
    switch (m.type) {
    case 'progress':
      if (s.ready) s.error = s.error || new Error('progress after ready');
      s.progress.push(m);
      break;
    case 'ready':
      if (s.states.length) s.error = s.error || new Error('ready after the first state');
      s.ready = m;
      break;
    case 'output':
      s.out += m.text;
      if (m.fd === 2) s.err += m.text;
      if (autoAck) { s.acked += m.text.length; s.w.postMessage({ type: 'ack', chars: m.text.length }); }
      break;
    case 'state':    s.states.push(m.state); break;
    case 'tick':     s.ticks++; break;
    case 'saved':    s.saved.push(m); break;
    case 'prepared': s.prepared = m; break;
    case 'superseded': s.superseded++; break;
    case 'file':     s.files.push(m); break;
    case 'exit':     s.exit = m.code; break;
    case 'crash':    s.crash = m; break;
    }
  });
  s.post = m => s.w.postMessage(m);
  s.mark = () => { s.markAt = s.out.length; s.errAt = s.err.length; s.stateAt = s.states.length; s.msgAt = s.msgs.length; };
  s.since = () => s.out.slice(s.markAt);
  s.errSince = () => s.err.slice(s.errAt);
  s.msgsSince = () => s.msgs.slice(s.msgAt);
  s.send = t => { s.mark(); s.post({ type: 'input', text: t }); };
  s.alive = () => {
    if (s.crash) throw new Error('crash: ' + s.crash.message);
    if (s.error) throw new Error('worker error: ' + s.error);
    if (s.exit !== null) throw new Error('exit ' + s.exit);
  };
  s.expectOut = (re, ms) => waitFor('output ' + re, () => { s.alive(); return re.test(s.since()); }, ms);
  s.prompt = async ms => {
    await s.expectOut(/st> $/, ms);
    await waitFor('WAITING', () => s.states.length > s.stateAt && s.states[s.states.length - 1] === WAITING, ms);
  };
  s.started = async () => {
    await waitFor('ready', () => s.ready || s.crash || s.error, 60000);
    s.alive();
    await s.prompt(60000);
  };
  s.reply = (type, id, ms) => waitFor(type + ' reply ' + id,
    () => s.msgs.find(m => m.id === id && (m.type === type || m.type === 'error')), ms);
  s.close = () => s.w.terminate();
  s.post(Object.assign({ type: 'init', wasmModule, manifestUrl: 'manifest.json?v=' + encodeURIComponent(manifest.build),
                         build: manifest.build, mode: 'console', persist: true }, init));
  current = s;
  sessions.push(s);
  return s;
}

async function check(name, f) {
  try {
    await f();
    passes++;
    console.log('ok - ' + name);
  } catch (e) {
    failures++;
    console.log('not ok - ' + name + '\n  ' + String((e && e.message) || e));
    if (current) console.log('  output since the last input:\n  | ' +
                             current.since().slice(-600).split('\n').join('\n  | '));
  }
}

// a result line, right after the input or after the prompt of an earlier line
const val = v => new RegExp('(^|> )' + v.replace(/[.*+?^${}()|[\]\\]/g, '\\$&') + '\n', 'm');
const fetched = (s, name) => s.log.includes('/' + manifest.files.find(f => f.path === name).url);
const imageName = manifest.image, changesName = imageName.replace(/\.image$/, '.changes');
const sourcesName = manifest.files.find(f => f.path.endsWith('.sources')).path;
// a file of the manifest, inflated
const manifestFile = name => zlib.gunzipSync(fs.readFileSync(path.join(webDir, manifest.files.find(f => f.path === name).url)));

(async () => {
  const t0 = now();
  await startServer();
  let S;

  await check('1 progress, then ready before the first state; the first prompt', async () => {
    S = session();
    await S.started();
    const p = S.progress, fetches = p.filter(m => m.phase === 'fetch');
    const total = manifest.files.reduce((n, f) => n + f.size, 0);
    assert(fetches.length >= 10, fetches.length + ' fetch progress messages');
    assert(fetches.every(m => m.total === total), 'fetch total is the size of the files');
    assert(fetches.every((m, i) => !i || m.loaded >= fetches[i - 1].loaded), 'loaded grows');
    assert(fetches[fetches.length - 1].loaded === total, 'all of it loaded');
    assert(p[p.length - 1].phase === 'boot', 'boot comes last');
    const r = S.ready;
    assert(r.image === '/pharo/' + imageName && r.source === 'download' && r.persisted === false &&
           r.world === !!manifest.world && r.prepared === !!manifest.world && !r.storageError,
           'ready ' + JSON.stringify(r));
    assert(S.out === 'st> ', 'output ' + JSON.stringify(S.out));
    for (const f of manifest.files) assert(fetched(S, f.path), f.path + ' fetched');
    for (const st of manifest.st) assert(S.log.includes('/' + st), st + ' fetched');
  });
  if (!S || !S.ready) { console.log('cannot continue'); process.exit(1); }

  await check('2 3+4 gives 7; an error goes to fd 2; WebDisplay is not available', async () => {
    S.send('3+4\n');
    await S.prompt();
    assert(S.since() === '7\nst> ', 'output ' + JSON.stringify(S.since()));
    S.send('nil foo\n');
    await S.prompt();
    assert(/^Error: MessageNotUnderstood/.test(S.errSince()), 'stderr ' + JSON.stringify(S.errSince()));
    assert(S.msgsSince().some(m => m.type === 'output' && m.fd === 2), 'output {fd: 2}');
    S.send('(Smalltalk at: #OSWebDriver ifAbsent: [ nil ]) ifNil: [ #none ] ifNotNil: [ :c | c isSuitable ]\n');
    await S.prompt();
    assert(S.since() === (manifest.world ? 'false' : '#none') + '\nst> ', 'output ' + JSON.stringify(S.since()));
  });

  await check('3 input sent before ready is evaluated after it', async () => {
    const E = session({ persist: false });
    try {
      E.post({ type: 'input', text: '6 * 7\n' });
      assert(!E.ready, 'posted before ready');
      await waitFor('ready', () => E.ready || E.crash, 60000);
      await E.expectOut(val('42'), 60000);
      assert(E.out === 'st> 42\nst> ', 'output ' + JSON.stringify(E.out));
    } finally { await E.close(); }
  });

  await check('4 a busy loop ticks; interrupt stops it: interrupted {registered}, Interrupted. on fd 2', async () => {
    S.send('[true] whileTrue\n');
    await sleep(2600);
    S.alive();
    const states = S.msgsSince().filter(m => m.type === 'state').length;
    assert(S.ticks >= 2, S.ticks + ' ticks in 2.6 s');
    assert(states <= 3, states + ' state messages while busy');
    const t = now();
    S.mark();
    S.post({ type: 'interrupt' });
    await S.expectOut(/^Interrupted\.\n/, 5000);
    const reply = S.msgsSince().find(m => m.type === 'interrupted');
    assert(reply && reply.registered === true, 'interrupted ' + JSON.stringify(reply));
    console.log(`#   ${S.ticks} ticks, ${states} states; stopped in ${(now() - t).toFixed(0)} ms`);
    await S.prompt();
    S.send('3 + 4\n');
    await S.expectOut(val('7'));
    await S.prompt();
  });

  await check('5 fs: writeFile, listDir, readFile, remove; an error for a missing file', async () => {
    const data = new TextEncoder().encode('héllo from the page');
    S.post({ type: 'fs', id: 51, op: 'writeFile', path: '/pharo/up.txt', data });
    assert((await S.reply('fs-result', 51)).type === 'fs-result', 'writeFile answered');
    S.post({ type: 'fs', id: 52, op: 'listDir', path: '/pharo' });
    const d = await S.reply('fs-result', 52);
    for (const name of ['up.txt', imageName, changesName, sourcesName, 'st'])
      assert(d.entries.includes(name), name + ' listed: ' + JSON.stringify(d.entries));
    assert(!d.entries.includes('.'), 'no . entry');
    S.send("'up.txt' asFileReference contents\n");
    await S.expectOut(val("'héllo from the page'"));
    await S.prompt();
    S.send("'from-image.txt' asFileReference writeStreamDo: [ :s | s nextPutAll: 'äbc' ]. #written\n");
    await S.expectOut(val('#written'));
    await S.prompt();
    S.post({ type: 'fs', id: 53, op: 'readFile', path: '/pharo/from-image.txt' });
    const f = await S.reply('fs-result', 53);
    assert(f.type === 'fs-result' && new TextDecoder().decode(f.data) === 'äbc', 'read ' + JSON.stringify(f));
    S.post({ type: 'fs', id: 57, op: 'writeFile', path: '/pharo/text.txt', data: 'plain text' });
    await S.reply('fs-result', 57);
    S.post({ type: 'fs', id: 58, op: 'readFile', path: '/pharo/text.txt' });
    const tf = await S.reply('fs-result', 58);
    assert(tf.type === 'fs-result' && new TextDecoder().decode(tf.data) === 'plain text', 'a string written');
    S.post({ type: 'fs', id: 54, op: 'remove', path: '/pharo/up.txt' });
    assert((await S.reply('fs-result', 54)).type === 'fs-result', 'remove answered');
    S.post({ type: 'fs', id: 55, op: 'listDir', path: '/pharo' });
    assert(!(await S.reply('fs-result', 55)).entries.includes('up.txt'), 'up.txt removed');
    S.post({ type: 'fs', id: 56, op: 'readFile', path: '/pharo/no-such-file' });
    const e = await S.reply('fs-result', 56);
    assert(e.type === 'error' && e.message, 'error with a message, got ' + JSON.stringify(e));
  });

  await check('6 output pauses above 1 MiB unacked, and resumes below 512 KiB', async () => {
    const B = session({ persist: false }, { autoAck: false });
    try {
      await waitFor('ready', () => B.ready || B.crash, 60000);
      await waitFor('first prompt', () => /st> $/.test(B.out), 60000);
      B.send('1 to: 50000 do: [ :i | Stdio stdout nextPutAll: (String new: 99 withAll: $x); lf ]. #done\n');
      const settle = async () => {      // until the output stops growing
        let n = -1;
        for (let k = 0; k < 100 && n !== B.out.length; k++) { n = B.out.length; await sleep(300); }
        return n;
      };
      const stalled = await settle();
      B.alive();
      assert(!/#done/.test(B.out), 'stalled before the end');
      assert(stalled >= (1 << 20) && stalled < (1 << 20) + 600000,
             'stalled just above 1 MiB unacked (' + stalled + ' chars)');
      // 600 KiB still unacked: still paused
      B.post({ type: 'ack', chars: stalled - 600 * 1024 });
      await sleep(600);
      assert(B.out.length === stalled, 'still paused above 512 KiB');
      B.post({ type: 'ack', chars: 200 * 1024 });
      await waitFor('more output', () => B.out.length > stalled, 5000);
      console.log(`#   stalled at ${stalled} chars`);
      const t = setInterval(() => B.post({ type: 'ack', chars: 1 << 30 }), 20);
      try { await waitFor('done', () => /#done\nst> $/.test(B.out), 120000); } finally { clearInterval(t); }
      // 'st> ' and the first line, 49999 more, '#done', 'st> '
      const lines = B.out.split('\n');
      assert(lines.length === 50002 && lines.slice(1, 50000).every(l => l.length === 99),
             (lines.length - 2) + ' lines');
    } finally { await B.close(); }
  });

  await check('7 Smalltalk exit: 3 gives exit {code: 3}; then download still answers the files', async () => {
    const X = session({ persist: false });
    try {
      await X.started();
      X.send('Transcript show: \'bye\'; cr. Smalltalk exit: 3\n');
      await waitFor('exit', () => X.exit !== null || X.crash, 30000);
      assert(X.exit === 3, 'exit code ' + X.exit);
      assert(/bye\n/.test(X.since()), 'output before the exit: ' + JSON.stringify(X.since()));
      X.post({ type: 'download', id: 71 });
      await waitFor('two files', () => X.files.length === 2 || X.msgs.find(m => m.type === 'error'));
      const [image, changes] = X.files;
      assert(image.name === imageName && changes.name === changesName && image.count === 2,
             'names ' + image.name + ' ' + changes.name);
      assert(image.data.length === sizeOf(imageName), 'image of ' + image.data.length + ' bytes');
      assert(changes.data.length >= sizeOf(changesName), 'changes of ' + changes.data.length + ' bytes');
    } finally { await X.close(); }
  });

  await check('8 an upload of a truncated image: crash {message}, the slot as it was; resetStorage still answers', async () => {
    const gz = fs.readFileSync(path.join(webDir, manifest.files.find(f => f.path === imageName).url));
    const image = zlib.gunzipSync(gz).subarray(0, 1 << 20);
    // a slot that the upload must not replace, as it does not boot
    const slot = await PharoStorage.open(memory).save(new Uint8Array(7), new Uint8Array(3), { build: 'earlier' });
    const X = session({ upload: { image, changes: new Uint8Array(0) } });
    X.crashExpected = true;
    try {
      await waitFor('crash', () => X.crash || X.exit !== null, 60000);
      const message = (X.crash ? X.crash.message : '') + X.err;
      assert(X.crash || X.exit !== 0, 'crash or a failing exit, got exit ' + X.exit);
      assert(message.trim(), 'a message');
      console.log('#   ' + (X.crash ? 'crash: ' + X.crash.message.split('\n')[0] : 'exit ' + X.exit) +
                  ' / ' + X.err.trim().split('\n').join(' / '));
      assert(!fetched(X, imageName) && fetched(X, sourcesName), 'the upload replaced the fetch of the image');
      // persisted: a slot is there, which the upload would replace
      assert(X.ready && X.ready.source === 'upload' && X.ready.persisted === true, 'ready ' + JSON.stringify(X.ready));
      assert(!X.saved.length, 'saved ' + JSON.stringify(X.saved));
      assert(memory.map.get('meta').id === slot.id && memory.map.get('Pharo.image').size === 7, 'the slot was replaced');
      X.post({ type: 'resetStorage', id: 81 });
      const r = await X.reply('reset', 81);
      assert(r.type === 'reset' && !r.error, 'reset ' + JSON.stringify(r));
      assert(memory.map.size === 0, 'the store is empty');
    } finally { await X.close(); }
  });

  await check('9 gzip skip: files sent with Content-Encoding: gzip, inflated by fetch, boot too', async () => {
    const G = session({ persist: false }, { encodeGzip: true });
    try {
      await G.started();
      G.send('3+4\n');
      await G.prompt();
      assert(G.since() === '7\nst> ', 'output ' + JSON.stringify(G.since()));
      assert(fetched(G, imageName), 'fetched');
    } finally { await G.close(); }
  });

  let saved = null;
  await check('10 save: saved {bytes}, kept in the store; a new worker restores it without fetching it', async () => {
    S.send('Smalltalk at: #HarnessMarker put: 4242\n');
    await S.prompt();
    S.mark();
    S.post({ type: 'save' });
    await S.prompt(60000);
    assert(S.since() === 'a SnapshotOperation\nst> ', 'output ' + JSON.stringify(S.since()));
    await waitFor('saved', () => S.saved.length, 30000);
    const m = S.saved[0];
    assert(!m.error && m.bytes > 1e7 && m.prepared === !!manifest.world, 'saved ' + JSON.stringify(m));
    const meta = memory.map.get('meta');
    assert(meta && meta.image === 'Pharo.image' && meta.imageSize === m.bytes && meta.build === manifest.build,
           'meta ' + JSON.stringify(meta));
    saved = new Uint8Array(await memory.map.get('Pharo.image').arrayBuffer());
    assert(saved.length === m.bytes, 'the stored image');
    const R = session();
    try {
      await R.started();
      assert(R.ready.source === 'saved' && R.ready.persisted === true && R.ready.savedAt === meta.savedAt,
             'ready ' + JSON.stringify(R.ready));
      assert(R.progress.some(p => p.phase === 'restore'), 'restore progress');
      assert(!fetched(R, imageName) && !fetched(R, changesName), 'the image and its .changes were not fetched');
      assert(fetched(R, sourcesName), 'the .sources was fetched');
      R.send('Smalltalk at: #HarnessMarker\n');
      await R.expectOut(val('4242'));
      await R.prompt();
      current = R;

      // the .changes of the slot follows: after SYNC_MS, on flush, at the exit
      const stored = () => memory.map.get('Pharo.changes').size;
      const before = stored();
      R.send("Object compile: 'harnessProbeA ^ 1' classified: 'harness'. #a\n");
      await R.expectOut(val('#a'));
      await R.prompt();
      const t = now();
      await waitFor('the .changes stored again', () => stored() > before, 12000);
      console.log(`#   the .changes was stored again after ${(now() - t).toFixed(0)} ms`);
      const afterA = stored();
      R.send("Object compile: 'harnessProbeB ^ 2' classified: 'harness'. #b\n");
      await R.expectOut(val('#b'));
      await R.prompt();
      R.post({ type: 'flush' });
      await waitFor('flush', () => stored() > afterA, 1500);
      const afterB = stored();
      R.send("Object compile: 'harnessProbeC ^ 3' classified: 'harness'. #c\n");
      await R.expectOut(val('#c'));
      R.post({ type: 'eof' });
      await waitFor('exit', () => R.exit !== null || R.crash, 30000);
      assert(R.exit === 0, 'exit ' + R.exit);
      assert(stored() > afterB, 'the .changes was stored before exit');
      const changes = new TextDecoder().decode(new Uint8Array(await memory.map.get('Pharo.changes').arrayBuffer()));
      assert(['harnessProbeA', 'harnessProbeB', 'harnessProbeC'].every(p => changes.includes(p)), 'the methods are in it');
    } finally { await R.close(); current = S; }
  });

  await check('11 a store that refuses: saved {error} says Download, and the REPL goes on', async () => {
    storeFails = Object.assign(new Error('the quota is exceeded'), { name: 'QuotaExceededError' });
    try {
      const n = S.saved.length;
      S.post({ type: 'save' });
      await waitFor('saved', () => S.saved.length > n, 60000);
      const m = S.saved[n];
      assert(m.error && /Download/.test(m.error) && /no room/.test(m.error), 'saved ' + JSON.stringify(m));
      await S.prompt(30000);
      S.send('3 + 4\n');
      await S.expectOut(val('7'));
      await S.prompt();
    } finally { storeFails = null; }
  });

  await check('12 upload: boots the uploaded image, which becomes the slot once it prompts', async () => {
    assert(saved, 'no saved image');
    memory.map.clear();
    const U = session({ upload: { image: saved, changes: new TextEncoder().encode('') } });
    try {
      await waitFor('ready', () => U.ready || U.crash || U.error, 60000);
      U.alive();
      assert(U.ready.source === 'upload' && U.ready.persisted === false && U.ready.prepared === !!manifest.world,
             'ready ' + JSON.stringify(U.ready));
      assert(!memory.map.size, 'nothing stored before the first prompt');
      await U.prompt(60000);
      await waitFor('saved {upload}', () => U.saved.length, 30000);
      const m = U.saved[0];
      assert(m.upload === true && !m.error && m.bytes === saved.length && m.prepared === !!manifest.world,
             'saved ' + JSON.stringify(m));
      assert(U.msgs.indexOf(m) > U.msgs.findIndex(x => x.type === 'state' && x.state === WAITING), 'after the first WAITING');
      assert(!fetched(U, imageName) && !fetched(U, changesName), 'nothing fetched for the image');
      U.send('Smalltalk at: #HarnessMarker\n');
      await U.expectOut(val('4242'));
      await U.prompt();
      assert(sha256(new Uint8Array(await memory.map.get('Pharo.image').arrayBuffer())) === sha256(saved),
             'the slot holds the upload');
      // and it is this VM's own: its .changes is stored again
      const before = memory.map.get('Pharo.changes').size;
      U.send("Object compile: 'harnessProbeU ^ 5' classified: 'harness'. #u\n");
      await U.expectOut(val('#u'));
      await U.prompt();
      U.post({ type: 'flush' });
      await waitFor('the .changes stored', () => memory.map.get('Pharo.changes').size > before, 3000);
    } finally { await U.close(); }
  });

  await check('13 resetStorage empties the store; the next worker downloads the image; a damaged slot is still persisted', async () => {
    S.post({ type: 'resetStorage', id: 131 });
    const r = await S.reply('reset', 131);
    assert(r.type === 'reset' && !r.error, 'reset ' + JSON.stringify(r));
    assert(memory.map.size === 0, 'store emptied: ' + [...memory.map.keys()]);
    const D = session();
    try {
      await D.started();
      assert(D.ready.source === 'download' && D.ready.persisted === false, 'ready ' + JSON.stringify(D.ready));
      assert(fetched(D, imageName), 'the image was fetched');
      D.send('Smalltalk at: #HarnessMarker ifAbsent: [ #none ]\n');
      await D.expectOut(val('#none'));
    } finally { await D.close(); }
    // a slot whose image cannot be read: the image is downloaded, and
    // persisted says that there is a slot, which resetStorage deletes
    await memory.write([['Pharo.image', 'not an image'], ['Pharo.changes', new Uint8Array(1)],
                        ['meta', { id: 'damaged', image: 'Pharo.image', changes: 'Pharo.changes' }]]);
    const E = session();
    try {
      await E.started();
      assert(E.ready.source === 'download' && E.ready.persisted === true &&
             /^the saved image could not be read \(the saved image is damaged\)$/.test(E.ready.storageError),
             'ready ' + JSON.stringify(E.ready));
      assert(fetched(E, imageName), 'the image was fetched');
      E.post({ type: 'resetStorage', id: 132 });
      const e = await E.reply('reset', 132);
      assert(e.type === 'reset' && !e.error && memory.map.size === 0, 'reset ' + JSON.stringify(e));
    } finally { await E.close(); }
  });

  await check('14 init.display: PharoDisplay gives the webDisplay and handles the display messages', async () => {
    const W = session({ persist: false, display: { canvas: 'stub' } }, { display: true });
    try {
      await W.started();
      const created = W.msgs.find(m => m.type === 'display' && m.kind === 'created');
      assert(created && created.init.canvas === 'stub', 'created ' + JSON.stringify(created));
      assert(W.msgs.indexOf(created) < W.msgs.indexOf(W.ready), 'created before ready');
      // the display gets the driver before the first slice, whatever the page sends
      const attach = W.msgs.find(m => m.type === 'display' && m.kind === 'handle');
      assert(attach && attach.what === 'attach' && attach.state === 0 && attach.dead === false &&
             W.msgs.indexOf(attach) < W.msgs.indexOf(W.ready), 'attach ' + JSON.stringify(attach));
      W.mark();
      W.post({ type: 'display', what: 'ping' });
      const handled = await waitFor('handled', () => W.msgsSince().find(m => m.type === 'display' && m.kind === 'handled'));
      assert(handled.what === 'ping', 'handled ' + JSON.stringify(handled));
      W.send('(Smalltalk at: #OSWebDriver ifAbsent: [ nil ]) ifNil: [ #none ] ifNotNil: [ :c | c isSuitable ]\n');
      await W.prompt();
      assert(W.since() === (manifest.world ? 'true' : '#none') + '\nst> ', 'output ' + JSON.stringify(W.since()));
    } finally { await W.close(); }
  });

  await check('15 two workers on one slot: once one saves, the other stops storing its .changes', async () => {
    // Two tabs, or the Console and the world: the image of a slot reads its
    // sources at offsets of its own .changes, so the .changes of a VM that
    // runs an older image must not replace it
    memory.map.clear();
    const P = session();
    let Q = null;
    const stored = () => memory.map.get('Pharo.changes');
    const sourceOf = async (s, selector) => {
      s.send('(Object >> #' + selector + ') sourceCode\n');
      await s.prompt();
      return s.since();
    };
    try {
      await P.started();
      P.post({ type: 'save' });
      await waitFor('saved', () => P.saved.length, 60000);
      await P.prompt(60000);
      Q = session();
      await Q.started();
      assert(Q.ready.source === 'saved' && Q.ready.persisted, 'Q boots from the slot of P: ' + JSON.stringify(Q.ready));
      current = P;
      P.send("Object compile: 'tabProbeP ^ ''the source of P''' classified: 'harness'. #p\n");
      await P.expectOut(val('#p'));
      await P.prompt();
      P.post({ type: 'save' });
      await waitFor('saved again', () => P.saved.length > 1, 60000);
      await P.prompt(60000);
      assert(!P.saved[1].error, 'saved ' + JSON.stringify(P.saved[1]));
      const slot = memory.map.get('meta'), changes = stored();
      // Q compiles a method at the offset where P's is, in its own .changes
      current = Q;
      Q.send("Object compile: 'tabProbeQ ^ ''Q wrote this, longer than the method of P, at the same offset''' " +
             "classified: 'harness'. #q\n");
      await Q.expectOut(val('#q'));
      await Q.prompt();
      const t = now();
      await waitFor('superseded', () => Q.superseded, 12000);
      console.log(`#   Q superseded after ${(now() - t).toFixed(0)} ms`);
      Q.post({ type: 'flush' });
      await sleep(500);
      assert(memory.map.get('meta').id === slot.id && stored() === changes, 'the slot is still the one of P');
      assert(!P.superseded, 'P was not superseded');
      // P still owns it
      current = P;
      P.send("Object compile: 'tabProbeP2 ^ 2' classified: 'harness'. #p2\n");
      await P.expectOut(val('#p2'));
      await P.prompt();
      P.post({ type: 'flush' });
      await waitFor('the .changes of P stored', () => stored() !== changes, 3000);
      const R = session();
      try {
        await R.started();
        const got = await sourceOf(R, 'tabProbeP');
        assert(/the source of P/.test(got), 'the source in the slot: ' + JSON.stringify(got));
      } finally { await R.close(); }
      // Q saves: the slot is Q's now, and P is the one superseded
      current = Q;
      Q.post({ type: 'save' });
      await waitFor('Q saved', () => Q.saved.length, 60000);
      await Q.prompt(60000);
      assert(!Q.saved[0].error && memory.map.get('meta').id !== slot.id, 'the slot of Q');
      const ofQ = stored();
      current = P;
      P.send("Object compile: 'tabProbeP3 ^ 3' classified: 'harness'. #p3\n");
      await P.expectOut(val('#p3'));
      await P.prompt();
      P.post({ type: 'flush' });
      await waitFor('P superseded', () => P.superseded, 3000);
      assert(stored() === ofQ, 'the .changes of Q stays');
      const T = session();
      try {
        await T.started();
        const got = await sourceOf(T, 'tabProbeQ');
        assert(/Q wrote this/.test(got), 'the source in the slot: ' + JSON.stringify(got));
      } finally { await T.close(); }
    } finally {
      await P.close();
      if (Q) await Q.close();
      current = S;
    }
  });

  await check('16 a database that cannot be opened: ready {storageError}; Save says Download; the REPL goes on', async () => {
    storeUnavailable = Object.assign(new Error('The operation is insecure.'), { name: 'InvalidStateError', unavailable: true });
    const N = session();
    try {
      await N.started();
      const r = N.ready;
      assert(r.source === 'download' && !r.persisted &&
             /^this browser does not keep images here \(The operation is insecure\.\); use Download/.test(r.storageError),
             'ready ' + JSON.stringify(r));
      N.post({ type: 'save' });
      await waitFor('saved', () => N.saved.length, 60000);
      assert(N.saved[0].error && /Download/.test(N.saved[0].error), 'saved ' + JSON.stringify(N.saved[0]));
      await N.prompt(60000);
      N.send('3 + 4\n');
      await N.expectOut(val('7'));
      await N.prompt();
    } finally { storeUnavailable = null; await N.close(); }
  });

  await check('17 vm-storage.js: guarded writes, a fresh id per save, meta, a load during a save, an IndexedDB that fails to open', async () => {
    const m = PharoStorage.memory(), st = PharoStorage.open(m), bytes = n => new Uint8Array(n);
    assert(await st.meta() === null, 'no meta in an empty store');
    const a = await st.save(bytes(3), bytes(2), { build: 'x' });
    const b = await st.save(bytes(3), bytes(2), { build: 'x' });
    assert(a.id && b.id && a.id !== b.id, 'ids ' + a.id + ' ' + b.id);
    assert(JSON.stringify(await st.meta()) === JSON.stringify(b), 'meta is that of the last save');
    assert(await st.syncChanges(bytes(5), a) === null, 'the slot of a is gone');
    assert(m.map.get('Pharo.changes').size === 2, 'nothing written');
    const b2 = await st.syncChanges(bytes(7), b);
    assert(b2 && b2.id === b.id && b2.changesSize === 7 && m.map.get('meta').changesSize === 7, 'b synced');
    await st.reset();
    assert(await st.syncChanges(bytes(9), b2) === null && !m.map.size, 'nothing after a reset');
    assert(await st.meta() === null, 'no meta after a reset');
    // another page saves between the reads of load: it reads that slot, whole
    await st.save(bytes(1), bytes(1), {});
    let raced = false;
    const racing = Object.assign({}, m, { get: async key => {
      const v = await m.get(key);
      if (key === 'Pharo.image' && !raced) { raced = true; await st.save(bytes(4), bytes(4), {}); }
      return v;
    } });
    const got = await PharoStorage.open(racing).load();
    assert(raced && got.image.length === 4 && got.changes.length === 4 && got.meta.imageSize === 4,
           'load during a save: ' + (got && [got.image.length, got.changes.length]));
    const refused = { open() {
      const r = {};
      setTimeout(() => { r.error = Object.assign(new Error('The operation is insecure.'), { name: 'SecurityError' }); r.onerror(); });
      return r;
    } };
    let e = null;
    try { await PharoStorage.open(PharoStorage.indexedDB(refused)).load(); } catch (x) { e = x; }
    assert(e && e.unavailable === true && e.name === 'SecurityError' && /insecure/.test(e.message), 'load: ' + e);
    // the .sources of a slot: saved, loaded, kept as stored by a save of the
    // slot that has it, written again when another page saved meanwhile,
    // deleted by a save without one
    await st.reset();
    const src = { name: 'Other.sources', data: new Blob([bytes(11)]) };
    const s1 = await st.save(bytes(3), bytes(2), {}, src, null);
    const stored = m.map.get('Pharo.sources');
    assert(s1.sources === 'Pharo.sources' && s1.sourcesName === 'Other.sources' && s1.sourcesSize === 11 && stored.size === 11,
           'saved with its .sources: ' + JSON.stringify(s1));
    const l1 = await st.load();
    assert(l1.sources && l1.sources.name === 'Other.sources' && l1.sources.data.length === 11, 'loaded with it');
    const s2 = await st.save(bytes(4), bytes(2), {}, src, s1);
    assert(m.map.get('Pharo.sources') === stored && m.map.get('meta').id === s2.id && s2.sourcesSize === 11,
           'kept as stored when the slot had it');
    const other = await st.save(bytes(5), bytes(2), {}, null, null);
    assert(!m.map.has('Pharo.sources') && !other.sources, 'deleted by a save without one');
    const s3 = await st.save(bytes(4), bytes(2), {}, { name: 'Other.sources', data: bytes(11) }, s2);
    assert(m.map.get('Pharo.sources').size === 11 && s3.sourcesName === 'Other.sources', 'written again once superseded');
    const l3 = await st.load();
    assert(l3.image.length === 4 && l3.sources.data.length === 11, 'the slot of s3');
    m.map.delete('Pharo.sources');
    e = null;
    try { await st.load(); } catch (x) { e = x; }
    assert(e && /\.sources of the saved image is missing/.test(e.message), 'a missing .sources: ' + e);
  });

  await check('18 an upload with a .sources of its own: in /pharo, not fetched; the slot keeps it; a save does not write it again', async () => {
    memory.map.clear();
    // the .sources of the site, with a line more: the same name, another size
    const site = manifestFile(sourcesName);
    const own = Buffer.concat([site, Buffer.from('\r"harness: a .sources of its own"\r')]);
    const image = manifestFile(imageName), changes = manifestFile(changesName);
    const sizeOfSources = '(FileLocator imageDirectory / ' + JSON.stringify(sourcesName).replace(/"/g, "'") + ') size\n';
    // Blobs, as open-image.js gives them
    const U = session({ upload: { image: new Blob([image]), changes: new Blob([changes]),
                                  sources: { name: sourcesName, data: new Blob([own]) } } });
    try {
      await U.started();
      assert(U.ready.source === 'upload' && U.ready.sources === sourcesName, 'ready ' + JSON.stringify(U.ready));
      assert(!fetched(U, sourcesName) && !fetched(U, imageName), 'nothing fetched: ' + U.log.join(' '));
      U.send(sizeOfSources);
      await U.expectOut(val(String(own.length)));
      await U.prompt();
      U.send('(Object >> #printString) sourceCode lines first\n');
      await U.expectOut(val("'printString'"));
      await U.prompt();
      await waitFor('saved {upload}', () => U.saved.length, 30000);
      assert(!U.saved[0].error, 'saved ' + JSON.stringify(U.saved[0]));
      const meta = memory.map.get('meta');
      assert(meta.sourcesName === sourcesName && meta.sourcesSize === own.length &&
             memory.map.get('Pharo.sources').size === own.length, 'the slot keeps it: ' + JSON.stringify(meta));
    } finally { await U.close(); }
    const R = session();
    try {
      await R.started();
      current = R;
      assert(R.ready.source === 'saved' && R.ready.sources === sourcesName, 'ready ' + JSON.stringify(R.ready));
      assert(!fetched(R, sourcesName), 'the .sources of the site is not fetched: ' + R.log.join(' '));
      R.send(sizeOfSources);
      await R.expectOut(val(String(own.length)));
      await R.prompt();
      const stored = memory.map.get('Pharo.sources');
      R.post({ type: 'save' });
      await waitFor('saved', () => R.saved.length, 60000);
      await R.prompt(60000);
      const meta = memory.map.get('meta');
      assert(!R.saved[0].error && meta.sourcesName === sourcesName, 'saved ' + JSON.stringify(R.saved[0]));
      assert(memory.map.get('Pharo.sources') === stored, 'the .sources was written again');
    } finally { await R.close(); current = S; }
    // the .sources of the site, given: not fetched, and not kept in the slot
    const M = session({ upload: { image, changes, sources: { name: sourcesName, data: site } } });
    try {
      await M.started();
      assert(M.ready.sources === sourcesName && !fetched(M, sourcesName), 'not fetched: ' + M.log.join(' '));
      await waitFor('saved {upload}', () => M.saved.length, 30000);
      assert(!memory.map.get('meta').sources && !memory.map.has('Pharo.sources'), 'the slot has no .sources of its own');
    } finally { await M.close(); }
  });

  const stockDir = path.join(webDir, '..', 'image', 'stock');
  const stock = manifest.world && fs.existsSync(path.join(stockDir, 'Pharo.image'))
    ? { image: fs.readFileSync(path.join(stockDir, 'Pharo.image')), changes: fs.readFileSync(path.join(stockDir, 'Pharo.changes')) }
    : null;
  if (!stock) console.log('# skip 19: no world image, or no stock image in ' + stockDir);
  else await check('19 init.prepare: a stock image in the world is prepared in the REPL, saved, and kept; or says why not', async () => {
    const world = { mode: 'world', prepare: true, display: { canvas: 'stub' } };
    // an OSWindow-Web.st that defines nothing: web-bootstrap.st fails, and
    // the image is not saved (but kept, as it booted)
    memory.map.clear();
    const F = session(Object.assign({ upload: stock }, world), { display: true, override: { '/st/OSWindow-Web.st': '' } });
    try {
      await waitFor('prepared', () => F.prepared || F.crash || F.exit !== null, 120000);
      assert(F.ready.preparing === true && F.ready.prepared === false, 'ready ' + JSON.stringify(F.ready));
      assert(!F.msgs.some(m => m.type === 'display'), 'no display while preparing');
      assert(F.prepared.saved === false && /OSWebDriver/.test(F.prepared.error || ''), 'prepared ' + JSON.stringify(F.prepared));
      await waitFor('saved {upload}', () => F.saved.length, 30000);
      await sleep(500);
      assert(F.saved.length === 1 && F.saved[0].upload && !F.saved[0].prepared && memory.map.get('meta').prepared === false,
             'only the upload was stored: ' + JSON.stringify(F.saved));
    } finally { await F.close(); }
    const t = now();
    const P = session(Object.assign({ upload: stock }, world), { display: true });
    try {
      current = P;
      await waitFor('prepared', () => P.prepared || P.crash || P.exit !== null, 300000);
      P.alive();
      assert(!P.prepared.error && P.prepared.saved === true, 'prepared ' + JSON.stringify(P.prepared));
      console.log(`#   prepared in ${((now() - t) / 1000).toFixed(1)} s`);
      const saves = P.saved.map(m => [!!m.upload, m.prepared, !!m.error]);
      assert(JSON.stringify(saves) === '[[true,false,false],[false,true,false]]', 'saved ' + JSON.stringify(P.saved));
      assert(P.msgs.indexOf(P.prepared) > P.msgs.indexOf(P.saved[1]), 'prepared after saved');
      assert(memory.map.get('meta').prepared === true, 'the slot is prepared');
    } finally { await P.close(); current = S; }
    const W = session(world, { display: true });
    try {
      await waitFor('ready', () => W.ready || W.crash, 60000);
      assert(W.ready.source === 'saved' && W.ready.prepared === true && W.ready.preparing === false,
             'ready ' + JSON.stringify(W.ready));
      assert(W.msgs.some(m => m.type === 'display' && m.kind === 'created'), 'a display');
    } finally { await W.close(); }
  });

  await S.close();
  // the crashes asked for excepted
  for (const s of sessions) {
    const text = s.out + s.diag + (s.crash ? s.crash.message : '');
    if (!s.crashExpected && ENGINE_ERRORS.test(text)) {
      failures++;
      console.log(`not ok - engine error in session ${s.n}: ` + text.match(ENGINE_ERRORS)[0]);
    }
  }
  fs.rmSync(path.dirname(displayStub), { recursive: true, force: true });
  console.log(`# ${passes} passed, ${failures} failed, ${passes + failures} cases in ` +
              ((now() - t0) / 1000).toFixed(1) + ' s');
  process.exit(failures ? 1 : 0);
})();
