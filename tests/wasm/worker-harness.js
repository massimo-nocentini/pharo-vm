// worker-harness.js - run the real vm-worker.js in worker_threads, without a browser
//
// usage: node [v8 flags] worker-harness.js WEB_DIR
//
// WEB_DIR is a built web directory (build-wasm/web), served over HTTP by
// packaging/emscripten/tools/serve.mjs as by `make wasm-serve'.  Each session
// runs WEB_DIR/vm-worker.js, unmodified, in a worker_threads Worker with a
// 1 MB stack (about what browsers give workers), behind the small shim of
// tests/wasm/lib/worker-shim.js for the Web Worker globals it uses: self,
// location, importScripts (the files of WEB_DIR, through
// vm.runInThisContext), postMessage and onmessage, fetch (of the server,
// relative to the worker's URL), and a fair MessageChannel.  node has the
// rest (DecompressionStream, Blob, TextDecoder).  The storage of
// vm-storage.js is an in-memory store of this process (PharoStorage
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
// image without OSWindow-Web (init.prepare).  It also checks the versions
// of OSWindow-Web (manifest.webPackage): an image saved in the Console with
// an older version keeps it, a slot of before the versions counts as 1, the
// next world boot prepares such an image once, a current slot boots the
// world directly, and a preparation that leaves another version ends in an
// error, not in a loop.  The placeholders of the libraries of the FFI
// (manifest.libraries) are empty files of /pharo, where CairoLibrary finds
// libcairo.so.2; and an image of version 2 (OSWindow-Web without its
// AthensCairoSurface extension: Athens needs the SurfacePlugin, which only
// a VM with SDL2 has, manifest.sdl2) is prepared to version 3 on its world
// boot, and then draws Roassal; an image of version 3 (Iceberg's stock
// remotes, scp-like URLs over SSH, which a browser cannot reach) is
// prepared to version 4 on its world boot, and then has Iceberg's https://
// remotes (remoteTypeSelector #httpsUrl), which the smart-HTTP transport of
// libgit2 clones (case 26, on every build: the setting is the image's, with
// or without libgit2).
// The notebook mode (W-NB1..4, init.mode 'notebook'): the kernel of
// st/web-notebook.st, its events (output {fd: 3, bytes}), a request posted
// before ready, Stop, the credit of the events, and a worker that boots the
// slot without ever storing anything.
// Prints every case and their count, and exits with
// status 1 if any fails.  Lane 70 (tests/wasm/lanes/70-worker-harness.sh)
// runs it.

'use strict';
const crypto = require('crypto');
const fs = require('fs');
const os = require('os');
const path = require('path');
const zlib = require('zlib');
const WorkerShim = require('./lib/worker-shim.js');

const webDir = path.resolve(process.argv[2] || 'build-wasm/web');
const srcDir = path.join(__dirname, '..', '..');
const manifest = JSON.parse(fs.readFileSync(path.join(webDir, 'manifest.json'), 'utf8'));
const wasmModule = new WebAssembly.Module(fs.readFileSync(path.join(webDir, 'pharo-web.wasm')));
const PharoStorage = require(path.join(webDir, 'vm-storage.js'));
const ENGINE_ERRORS = /RuntimeError|RangeError|Aborted|unreachable|signature_mismatch|unsupported syscall/;
const WAITING = 1, BUSY = 2;
const sizeOf = name => manifest.files.find(f => f.path === name).size;
// The version of OSWindow-Web of the site, as vm-worker.js takes it
const webPackage = Number.isInteger(manifest.webPackage) && manifest.webPackage || 1;
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

// ---- sessions

let failures = 0, passes = 0;
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
let shim = null;

// One worker.  `autoAck' acks every output message, as the page does after
// rendering it.
function session(init = {}, { autoAck = true, encodeGzip = false, display = false, override = null } = {}) {
  const site = shim.site({ encodeGzip, override });
  const s = { n: site.n, msgs: [], out: '', err: '', states: [], progress: [], ready: null, exit: null, crash: null,
              saved: [], prepared: null, superseded: 0, files: [], ticks: 0, error: null, diag: '', log: site.log,
              markAt: 0, errAt: 0, stateAt: 0, msgAt: 0, acked: 0, ev: [], evBytes: 0 };
  s.w = shim.worker(site, { overrides: display ? { 'display-worker.js': displayStub } : {} });
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
      // the events of the notebook kernel are bytes, acked as such
      if (m.fd === 3) {
        s.ev.push(m.bytes);
        s.evBytes += m.bytes.length;
        if (autoAck) { s.acked += m.bytes.length; s.w.postMessage({ type: 'ack', chars: m.bytes.length }); }
        break;
      }
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
// The events of a notebook worker so far, parsed: JSON lines, each one
// with "bytes": N followed by N bytes and a newline (its attachment); a
// line that does not parse fails the case
function events(s) {
  const all = Buffer.concat(s.ev), out = [];
  let at = 0;
  for (;;) {
    const lf = all.indexOf(10, at);
    if (lf < 0) return out;
    const ev = JSON.parse(all.subarray(at, lf).toString('utf8'));
    at = lf + 1;
    if (Number.isInteger(ev.bytes)) {
      if (all.length < at + ev.bytes + 1) return out;
      ev.attachment = all.subarray(at, at + ev.bytes);
      assert(all[at + ev.bytes] === 10, 'a newline after the attachment of ' + JSON.stringify(ev));
      at += ev.bytes + 1;
    }
    out.push(ev);
  }
}
// A request of the notebook kernel, as nb-kernel.js sends it
const request = (rid, code) =>
  ({ type: 'input', text: JSON.stringify({ op: 'run', rid, name: 'In[' + rid + ']', code }) + '\n' });
// a file of the manifest, inflated
const manifestFile = name => zlib.gunzipSync(fs.readFileSync(path.join(webDir, manifest.files.find(f => f.path === name).url)));

(async () => {
  const t0 = now();
  // storeFails makes the writes of the store fail, storeUnavailable all of
  // its operations
  shim = await WorkerShim.start(webDir, { memory, guard: op => {
    if (storeUnavailable) throw storeUnavailable;
    if (op === 'write' && storeFails) throw storeFails;
  } });
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
           r.world === !!manifest.world && r.prepared === !!manifest.world && !r.storageError &&
           r.webPackage === (manifest.world ? webPackage : 0) && r.fonts === (manifest.fonts || null),
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
    assert(!m.error && m.bytes > 1e7 && m.prepared === !!manifest.world &&
           m.webPackage === (manifest.world ? webPackage : 0), 'saved ' + JSON.stringify(m));
    const meta = memory.map.get('meta');
    assert(meta && meta.image === 'Pharo.image' && meta.imageSize === m.bytes && meta.build === manifest.build &&
           meta.webPackage === m.webPackage && meta.prepared === m.prepared, 'meta ' + JSON.stringify(meta));
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

  // ---- the notebook kernel (st/web-notebook.st; nb-kernel.js is its client)
  const nb = (init = {}, opts) => session(Object.assign({ mode: 'notebook' }, init), opts);
  const doneOf = (s, rid, ms) => waitFor('done ' + rid, () => {
    s.alive();
    return events(s).find(e => e.ev === 'done' && e.rid === rid);
  }, ms || 60000);

  let N = null;
  await check('W-NB1 a request posted before ready is served after hello; ready < hello < WAITING; nothing on fds 1 and 2', async () => {
    N = nb();
    current = N;
    N.post(request(1, '6 * 7'));
    assert(!N.ready, 'posted before ready');
    const done = await doneOf(N, 1);
    assert(done.status === 'ok' && JSON.stringify(done.values) === '["42"]', 'done ' + JSON.stringify(done));
    const r = N.ready;
    assert(r.mode === 'notebook' && r.source === (memory.map.has('meta') ? 'saved' : 'download') &&
           typeof r.gitHttp === 'boolean', 'ready ' + JSON.stringify(r));
    // the kernel says hello before it first waits for a request
    const iReady = N.msgs.indexOf(r);
    const iHello = N.msgs.findIndex(m => m.type === 'output' && m.fd === 3);
    const iWaiting = N.msgs.findIndex(m => m.type === 'state' && m.state === WAITING);
    assert(iReady >= 0 && iReady < iHello && iHello < iWaiting, `ready ${iReady} < hello ${iHello} < WAITING ${iWaiting}`);
    const hello = events(N)[0];
    assert(hello.ev === 'hello' && hello.proto === 1 && /^1[25]\.\d+$/.test(hello.version), 'hello ' + JSON.stringify(hello));
    assert(events(N).map(e => e.ev).join() === 'hello,start,done', 'events ' + events(N).map(e => e.ev));
    await waitFor('WAITING', () => N.states[N.states.length - 1] === WAITING);
    assert(!N.msgs.some(m => m.type === 'output' && m.fd !== 3), 'no output on fds 1 and 2: ' + JSON.stringify(N.out));
    console.log(`#   hello after ${N.msgs.filter(m => m.type === 'state').length} states, ${N.evBytes} bytes of events`);
  });

  if (N && N.ready) await check('W-NB2 interrupt stops an endless loop: interrupted {registered}, done interrupted; the next request answers', async () => {
    current = N;
    N.post(request(2, '[ true ] whileTrue'));
    await waitFor('start 2', () => { N.alive(); return events(N).find(e => e.ev === 'start' && e.rid === 2); });
    await sleep(500);
    N.mark();
    const t = now();
    N.post({ type: 'interrupt' });
    const done = await doneOf(N, 2, 5000);
    const reply = N.msgs.find(m => m.type === 'interrupted');
    assert(reply && reply.registered === true, 'interrupted ' + JSON.stringify(reply));
    assert(done.status === 'interrupted', 'done ' + JSON.stringify(done));
    console.log(`#   stopped in ${(now() - t).toFixed(0)} ms`);
    N.post(request(3, '3 + 4'));
    const next = await doneOf(N, 3);
    assert(next.status === 'ok' && JSON.stringify(next.values) === '["7"]', 'done ' + JSON.stringify(next));
    assert(!N.msgs.some(m => m.type === 'output' && m.fd !== 3), 'no output on fds 1 and 2: ' + JSON.stringify(N.out));
  });
  if (N) await N.close();
  current = S;

  await check('W-NB3 the events pause above 1 MiB of unacked bytes and resume below 512 KiB; an event of 300 KB and an attachment across messages are reassembled', async () => {
    const B = nb({ persist: false }, { autoAck: false });
    try {
      current = B;
      await waitFor('hello', () => { B.alive(); return B.ev.length; }, 60000);
      B.post({ type: 'ack', chars: B.evBytes });
      let acked = B.evBytes;
      // 80 displays of 50000 characters: 4 MB of events
      B.post(request(1, '1 to: 80 do: [ :i | Notebook show: (Notebook text: (String new: 50000 withAll: $x)) ]. 1'));
      const settle = async () => {      // until the events stop growing
        let n = -1;
        for (let k = 0; k < 100 && n !== B.evBytes; k++) { n = B.evBytes; await sleep(300); }
        return n;
      };
      const stalled = await settle() - acked;
      B.alive();
      const early = events(B);
      assert(!early.some(e => e.ev === 'done'), 'stalled before the end');
      // (the worker pauses the VM only between slices, and how many events
      // of 50 KB a slice writes depends on the speed of the machine: it
      // stalled at 1.05 to 3.2 MB in 14 runs, so no bound above but the end)
      assert(stalled >= (1 << 20), 'stalled above 1 MiB unacked (' + stalled + ' bytes)');
      assert(early.filter(e => e.ev === 'display').length < 80, 'stalled before the last display');
      // 600 KiB still unacked: still paused
      B.post({ type: 'ack', chars: stalled - 600 * 1024 });
      acked += stalled - 600 * 1024;
      const at = B.evBytes;
      await sleep(600);
      assert(B.evBytes === at, 'still paused above 512 KiB');
      B.post({ type: 'ack', chars: 200 * 1024 });
      await waitFor('more events', () => B.evBytes > at, 5000);
      console.log(`#   stalled at ${stalled} unacked bytes`);
      const timer = setInterval(() => B.post({ type: 'ack', chars: 1 << 30 }), 20);
      try {
        const done = await doneOf(B, 1, 120000);
        assert(done.status === 'ok', 'done ' + JSON.stringify(done));
        const shown = events(B).filter(e => e.ev === 'display');
        assert(shown.length === 80 && shown.every(e => e.rid === 1 && e.data === 'x'.repeat(50000)),
               shown.length + ' displays of 50000 x');
        // an event of 300 KB, and an attachment of 360000 bytes, which the
        // worker posts in more than one message (it coalesces up to 64 KB)
        B.post(request(2, 'Notebook text: (String new: 300000 withAll: $y)'));
        const big = await doneOf(B, 2, 60000);
        assert(big.status === 'ok' && big.values === null, 'done ' + JSON.stringify(big));
        const d = events(B).filter(e => e.ev === 'display' && e.rid === 2);
        assert(d.length === 1 && d[0].data === 'y'.repeat(300000), 'the display of 300000 y');
        const from = B.ev.length;
        B.post(request(3, '(Form extent: 300 @ 300 depth: 32) fillColor: Color red; yourself'));
        const form = await doneOf(B, 3, 60000);
        assert(form.status === 'ok' && form.values === null, 'done ' + JSON.stringify(form));
        const f = events(B).filter(e => e.ev === 'display' && e.rid === 3);
        assert(f.length === 1 && f[0].mime === 'image/x-pharo-bgra' && f[0].width === 300 && f[0].height === 300 &&
               f[0].attachment.length === 360000, 'the display of the Form: ' + JSON.stringify(f.map(e => e.bytes)));
        const px = f[0].attachment;
        let red = true;
        for (let i = 0; i < px.length; i += 4) red = red && px[i] === 0 && px[i + 1] === 0 && px[i + 2] === 255 && px[i + 3] === 255;
        assert(red, 'every pixel red, B G R A = 0 0 255 255: ' + Array.from(px.subarray(0, 8)));
        const messages = B.ev.length - from;
        assert(messages >= 2, 'the attachment came in ' + messages + ' message(s)');
        console.log(`#   the attachment of 360000 bytes in ${messages} messages`);
      } finally { clearInterval(timer); }
    } finally { await B.close(); current = S; }
  });

  await check('W-NB4 the notebook worker stores nothing: save, upload, prepare and a snapshot are ignored, resetStorage is refused', async () => {
    const before = new Map(memory.map), meta = JSON.stringify(memory.map.get('meta') || null);
    const gz = fs.readFileSync(path.join(webDir, manifest.files.find(f => f.path === imageName).url));
    // an upload that would not boot, which the kernel does not take
    const X = nb({ upload: { image: zlib.gunzipSync(gz).subarray(0, 1 << 20), changes: new Uint8Array(0) }, prepare: true });
    try {
      current = X;
      X.post(request(1, '3 + 4'));
      assert((await doneOf(X, 1)).status === 'ok', 'the first cell');
      assert(X.ready.source !== 'upload' && !X.ready.preparing, 'ready ' + JSON.stringify(X.ready));
      X.post({ type: 'save' });
      X.post({ type: 'resetStorage', id: 91 });
      const r = await X.reply('reset', 91);
      assert(r.type === 'reset' && r.error === 'not in notebook mode', 'reset ' + JSON.stringify(r));
      X.post(request(2, 'Smalltalk snapshot: true andQuit: false. 5'));
      const done = await doneOf(X, 2, 60000);
      assert(done.status === 'ok' && JSON.stringify(done.values) === '["5"]', 'done ' + JSON.stringify(done));
      X.post({ type: 'flush' });
      // longer than SETTLE_MS and SYNC_MS of the worker
      await sleep(6000);
      X.alive();
      const kinds = X.msgs.map(m => m.type);
      for (const k of ['storing', 'saved', 'edited', 'superseded', 'prepared'])
        assert(!kinds.includes(k), k + ' posted');
      assert(memory.map.size === before.size && [...before].every(([k, v]) => memory.map.get(k) === v) &&
             JSON.stringify(memory.map.get('meta') || null) === meta, 'the store changed: ' + [...memory.map.keys()]);
      assert(!X.msgs.some(m => m.type === 'output' && m.fd !== 3), 'no output on fds 1 and 2: ' + JSON.stringify(X.out));
      X.post(request(3, '6 * 7'));
      assert(JSON.stringify((await doneOf(X, 3)).values) === '["42"]', 'the kernel goes on');
    } finally { await X.close(); current = S; }
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
    const U = session({ upload: { image: saved, changes: new TextEncoder().encode('"VERSION:1.0"!') } });
    try {
      await waitFor('ready', () => U.ready || U.crash || U.error, 60000);
      U.alive();
      // (before the REPL runs, the version is the one in the bytes of the image)
      assert(U.ready.source === 'upload' && U.ready.persisted === false && U.ready.prepared === !!manifest.world &&
             U.ready.webPackage === (manifest.world ? webPackage : 0), 'ready ' + JSON.stringify(U.ready));
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
    // the version of OSWindow-Web of a slot, and of the slots of before the versions
    const versions = [{ webPackage: 3, prepared: true }, { webPackage: 1, prepared: false }, { webPackage: 0 },
                      { prepared: true }, { prepared: false }, {}, null, { webPackage: 'x', prepared: true }]
      .map(PharoStorage.webPackage);
    assert(JSON.stringify(versions) === '[3,1,0,1,0,null,null,1]', 'webPackage ' + JSON.stringify(versions));
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
      const saves = P.saved.map(m => [!!m.upload, m.prepared, m.webPackage, !!m.error]);
      assert(JSON.stringify(saves) === JSON.stringify([[true, false, 0, false], [false, true, webPackage, false]]),
             'saved ' + JSON.stringify(P.saved));
      assert(P.msgs.indexOf(P.prepared) > P.msgs.indexOf(P.saved[1]), 'prepared after saved');
      assert(P.prepared.webPackage === webPackage, 'prepared ' + JSON.stringify(P.prepared));
      const meta = memory.map.get('meta');
      assert(meta.prepared === true && meta.webPackage === webPackage, 'the slot is prepared: ' + JSON.stringify(meta));
    } finally { await P.close(); current = S; }
    const W = session(world, { display: true });
    try {
      await waitFor('ready', () => W.ready || W.crash, 60000);
      assert(W.ready.source === 'saved' && W.ready.prepared === true && W.ready.preparing === false,
             'ready ' + JSON.stringify(W.ready));
      assert(W.msgs.some(m => m.type === 'display' && m.kind === 'created'), 'a display');
    } finally { await W.close(); }
  });

  // ---- the versions of OSWindow-Web (manifest.webPackage)
  const versions = manifest.world && webPackage >= 2;
  if (!versions) console.log('# skip 20-23: ' + (manifest.world ? 'manifest.webPackage is ' + manifest.webPackage
                                                                : 'no world image'));
  const world = { mode: 'world', prepare: true, display: { canvas: 'stub' } };
  // the image of the site, made what an earlier site saved: OSWindow-Web
  // without the versions, and bitmap fonts
  const OLDER = 'OSWebDriver class removeSelector: #packageVersion; removeSelector: #packageMarker. ' +
                'FreeTypeSystemSettings loadFt2Library: false. StandardFonts setSmallBitmapFonts. #older\n';
  const FONT = '| f | f := StandardFonts defaultFont. ((f respondsTo: #realFont) ifTrue: [ f realFont ] ' +
               'ifFalse: [ f ]) class name\n';
  // the class of the default font that a preparation gives
  const fontClass = manifest.fonts === 'freetype' ? '#FreeTypeFont' : '#StrikeFont';
  // a slot of version 1, the one of case 20
  let older = null;

  if (versions) await check('20 a Console save of an image with an older OSWindow-Web keeps its version (1): not prepared', async () => {
    memory.map.clear();
    const C = session();
    try {
      current = C;
      await C.started();
      assert(C.ready.webPackage === webPackage && C.ready.prepared === true, 'ready ' + JSON.stringify(C.ready));
      C.send(OLDER);
      await C.expectOut(val('#older'));
      await C.prompt();
      C.post({ type: 'save' });
      await waitFor('saved', () => C.saved.length, 60000);
      await C.prompt(60000);
      const m = C.saved[0];
      assert(!m.error && m.webPackage === 1 && m.prepared === false, 'saved ' + JSON.stringify(m));
      const meta = memory.map.get('meta');
      assert(meta.webPackage === 1 && meta.prepared === false, 'meta ' + JSON.stringify(meta));
      older = new Map(memory.map);
    } finally { await C.close(); current = S; }
    // the Console boots it as it is, and does not prepare it
    const R = session();
    try {
      current = R;
      await R.started();
      assert(R.ready.source === 'saved' && R.ready.webPackage === 1 && R.ready.prepared === false &&
             R.ready.preparing === false, 'ready ' + JSON.stringify(R.ready));
      R.send('OSWebDriver class canUnderstand: #packageVersion\n');
      await R.expectOut(val('false'));
      await R.prompt();
      R.send(FONT);
      await R.expectOut(val('#StrikeFont'));
      await R.prompt();
    } finally { await R.close(); current = S; }
  });

  if (versions) await check('21 a preparation that leaves another version: prepared {error, webPackage}, once, and the REPL goes on', async () => {
    assert(older, 'no slot of version 1');
    memory.map.clear();
    for (const [k, v] of older) memory.map.set(k, v);
    // a web-bootstrap.st that leaves the package as it is
    const F = session(world, { display: true, override: { '/st/web-bootstrap.st': "'harness: left as it is' size" } });
    try {
      current = F;
      await waitFor('prepared', () => F.prepared || F.crash || F.exit !== null, 120000);
      F.alive();
      assert(F.ready.source === 'saved' && F.ready.preparing === true && F.ready.prepared === false &&
             F.ready.webPackage === 1, 'ready ' + JSON.stringify(F.ready));
      const p = F.prepared;
      assert(p.saved === true && p.webPackage === 1 &&
             p.error === 'web-bootstrap.st left version 1 of OSWindow-Web in the image, but this site has version ' +
                         webPackage, 'prepared ' + JSON.stringify(p));
      // no loop: it is said once, nothing is typed again, and the VM waits
      await sleep(2000);
      F.alive();
      assert(F.msgs.filter(m => m.type === 'prepared').length === 1, 'prepared once');
      assert(F.saved.length === 1 && F.saved[0].webPackage === 1 && !F.saved[0].prepared, 'saved ' + JSON.stringify(F.saved));
      assert(!F.msgs.some(m => m.type === 'display'), 'no display');
      assert(F.states[F.states.length - 1] === WAITING, 'the REPL waits: ' + F.states.slice(-3));
      const meta = memory.map.get('meta');
      assert(meta.webPackage === 1 && meta.prepared === false, 'meta ' + JSON.stringify(meta));
      F.send('3 + 4\n');
      await F.expectOut(val('7'));
    } finally { await F.close(); current = S; }
  });

  if (versions) await check('22 a slot of before the versions (prepared, no webPackage) is prepared once on the world boot, with its fonts', async () => {
    assert(older, 'no slot of version 1');
    memory.map.clear();
    for (const [k, v] of older) memory.map.set(k, v);
    // the meta that the site wrote before the versions
    const meta = Object.assign({}, memory.map.get('meta'), { prepared: true });
    delete meta.webPackage;
    memory.map.set('meta', meta);
    const t = now();
    const P = session(world, { display: true });
    try {
      current = P;
      await waitFor('prepared', () => P.prepared || P.crash || P.exit !== null, 300000);
      P.alive();
      console.log(`#   prepared again in ${((now() - t) / 1000).toFixed(1)} s`);
      assert(P.ready.source === 'saved' && P.ready.preparing === true && P.ready.prepared === false &&
             P.ready.webPackage === 1 && P.ready.fonts === (manifest.fonts || null), 'ready ' + JSON.stringify(P.ready));
      assert(!P.prepared.error && P.prepared.saved === true && P.prepared.webPackage === webPackage,
             'prepared ' + JSON.stringify(P.prepared));
      assert(P.saved.length === 1 && P.saved[0].prepared === true && P.saved[0].webPackage === webPackage,
             'saved ' + JSON.stringify(P.saved));
      const now2 = memory.map.get('meta');
      assert(now2.prepared === true && now2.webPackage === webPackage, 'meta ' + JSON.stringify(now2));
    } finally { await P.close(); current = S; }
    // the image has the package of the site, and the fonts of the preparation
    const R = session();
    try {
      current = R;
      await R.started();
      assert(R.ready.source === 'saved' && R.ready.webPackage === webPackage && R.ready.prepared === true,
             'ready ' + JSON.stringify(R.ready));
      R.send('OSWebDriver packageVersion\n');
      await R.expectOut(val(String(webPackage)));
      await R.prompt();
      R.send(FONT);
      await R.expectOut(val(fontClass));
      await R.prompt();
    } finally { await R.close(); current = S; }
  });

  if (versions) await check('23 a current slot opens the world directly', async () => {
    const W = session(world, { display: true });
    try {
      current = W;
      await waitFor('ready', () => W.ready || W.crash, 60000);
      W.alive();
      assert(W.ready.source === 'saved' && W.ready.prepared === true && W.ready.preparing === false &&
             W.ready.webPackage === webPackage, 'ready ' + JSON.stringify(W.ready));
      assert(W.msgs.some(m => m.type === 'display' && m.kind === 'created'), 'a display');
      await sleep(1000);
      W.alive();
      assert(!W.prepared && !W.saved.length, 'not prepared again: ' + JSON.stringify(W.saved));
    } finally { await W.close(); current = S; }
  });

  // ---- the libraries of the FFI
  const libraries = manifest.libraries || [];
  if (!libraries.length) console.log('# skip 24: manifest.libraries is ' + JSON.stringify(manifest.libraries));
  else await check(`24 the placeholders of the FFI, ${libraries.join(', ')}: empty files of /pharo; ` +
                    'CairoLibrary finds libcairo.so.2 there', async () => {
    const L = session({ persist: false });
    try {
      current = L;
      await L.started();
      L.post({ type: 'fs', id: 2401, op: 'listDir', path: '/pharo' });
      const d = await L.reply('fs-result', 2401);
      for (const [i, name] of libraries.entries()) {
        assert(d.entries.includes(name), name + ' listed: ' + JSON.stringify(d.entries));
        L.post({ type: 'fs', id: 2410 + i, op: 'readFile', path: '/pharo/' + name });
        const f = await L.reply('fs-result', 2410 + i);
        assert(f.type === 'fs-result' && f.data.length === 0, name + ' is empty: ' + JSON.stringify(f).slice(0, 200));
      }
      if (libraries.includes('libcairo.so.2')) {
        L.send('CairoLibrary uniqueInstance libraryName\n');
        await L.expectOut(val("'/pharo/libcairo.so.2'"));
        await L.prompt();
      }
    } finally { await L.close(); current = S; }
  });

  // ---- version 2 of OSWindow-Web to 3: the AthensCairoSurface extension
  const athens = versions && webPackage >= 3 && libraries.includes('libcairo.so.2') && stock;
  if (!athens) console.log('# skip 25: ' + (!versions ? 'no versions (see 20-23)'
                                           : webPackage < 3 ? 'manifest.webPackage is ' + webPackage
                                           : !stock ? 'no stock image in ' + stockDir : 'no libcairo.so.2'));
  // The methods of AthensCairoSurface in OSWindow-Web, one per line of
  // <file>: the selector, after 'class ' on the class side; answers how many
  const EXTENSION = file => '| out | out := OrderedCollection new. { AthensCairoSurface. AthensCairoSurface class } do: ' +
    '[ :c | (c methods select: [ :m | m protocolName asString asLowercase = \'*oswindow-web\' ]) do: [ :m | ' +
    'out add: (c isMeta ifTrue: [ \'class \' ] ifFalse: [ \'\' ]) , m selector ] ]. ' +
    `'${file}' asFileReference writeStreamDo: [ :s | out sorted do: [ :l | s nextPutAll: l; nextPut: Character lf ] ]. ` +
    'out size\n';
  // Athens on its own, and a blue Roassal box, whose centre is the centre
  // of its canvas: the red and yellow of an error morph cannot pass for it
  const SURFACE = '[ AthensCairoSurface extent: 4 @ 4. #drawn ] on: Error do: [ :e | e messageText ]\n';
  const ROASSAL = '| c f p | c := RSCanvas new. c add: (RSBox new size: 40; color: Color blue; yourself). ' +
    'f := (c createMorph extent: 100 @ 100; yourself) imageForm. p := f colorAt: 50 @ 50. ' +
    '{ p red. p green. p blue } collect: [ :x | (x * 255) rounded ]\n';
  if (athens) await check('25 an image of version 2, whose Athens needs the SurfacePlugin, is prepared to 3 on the world boot; then Roassal draws', async () => {
    memory.map.clear();
    // the site's image in the Console: which methods the extension has
    const C = session();
    let extension = null;
    try {
      current = C;
      await C.started();
      C.send(EXTENSION('/pharo/extension.txt'));
      await C.expectOut(/(^|> )[1-9][0-9]*\n/m);
      await C.prompt();
      C.post({ type: 'fs', id: 2501, op: 'readFile', path: '/pharo/extension.txt' });
      extension = new TextDecoder().decode((await C.reply('fs-result', 2501)).data);
      const methods = extension.split('\n').filter(l => l);
      assert(methods.includes('class registerSurface:') && methods.includes('asForm'), 'the extension: ' + methods);
      // their stock versions, from the stock image
      const K = session({ persist: false, upload: stock });
      const stockFiles = [];
      try {
        current = K;
        await K.started();
        for (const [i, method] of methods.entries()) {
          const [cls, selector] = method.startsWith('class ')
            ? ['AthensCairoSurface class', method.slice(6)] : ['AthensCairoSurface', method];
          K.send(`| m | m := ${cls} compiledMethodAt: #${selector} ifAbsent: [ nil ]. m ifNotNil: [ ` +
                 `'/pharo/stock-${i}.st' asFileReference writeStreamDo: [ :s | s nextPutAll: m sourceCode ]. ` +
                 `'/pharo/stock-${i}.protocol' asFileReference writeStreamDo: [ :s | s nextPutAll: m protocolName asString ] ]. ` +
                 `m isNil\n`);
          await K.expectOut(/(^|> )(true|false)\n/m);
          const absent = /(^|> )true\n/m.test(K.since());
          await K.prompt();
          const file = { cls, selector, absent, source: null, protocol: null };
          if (!absent) {
            for (const [key, ext] of [['source', 'st'], ['protocol', 'protocol']]) {
              K.post({ type: 'fs', id: 2510 + 2 * i + (key === 'source' ? 0 : 1), op: 'readFile', path: `/pharo/stock-${i}.${ext}` });
              file[key] = (await K.reply('fs-result', 2510 + 2 * i + (key === 'source' ? 0 : 1))).data;
            }
          }
          stockFiles.push(file);
        }
      } finally { await K.close(); current = C; }
      assert(stockFiles.some(f => !f.absent), 'the stock image has none of ' + methods);
      console.log(`#   the extension: ${methods.join(', ')}; the stock image lacks ` +
                  (stockFiles.filter(f => f.absent).map(f => f.selector).join(', ') || 'none of them'));
      // the image made what a site of version 2 saved: the stock methods
      // back in their package, and version 2
      for (const [i, f] of stockFiles.entries()) {
        if (f.absent) {
          C.send(`${f.cls} removeSelector: #${f.selector}. #removed\n`);
          await C.expectOut(val('#removed'));
        } else {
          for (const [key, ext] of [['source', 'st'], ['protocol', 'protocol']]) {
            C.post({ type: 'fs', id: 2550 + 2 * i + (key === 'source' ? 0 : 1), op: 'writeFile',
                     path: `/pharo/stock-${i}.${ext}`, data: f[key] });
            await C.reply('fs-result', 2550 + 2 * i + (key === 'source' ? 0 : 1));
          }
          C.send(`${f.cls} compile: '/pharo/stock-${i}.st' asFileReference contents ` +
                 `classified: '/pharo/stock-${i}.protocol' asFileReference contents. #restored\n`);
          await C.expectOut(val('#restored'));
        }
        await C.prompt();
      }
      C.send("OSWebDriver class compile: 'packageVersion ^ 2'; compile: 'packageMarker ^ #OSWindowWebPackage2'. " +
             'OSWebDriver packageVersion\n');
      await C.expectOut(val('2'));
      await C.prompt();
      C.send(EXTENSION('/pharo/extension-2.txt'));
      await C.expectOut(val('0'));
      await C.prompt();
      // (a VM with SDL2 has SurfacePlugin built in, cmake/plugins.cmake,
      // where the stock Athens draws too)
      C.send(SURFACE);
      await C.prompt();
      assert((manifest.sdl2 ? /(^|> )#drawn\n/m
                            : /(^|> )'Unable to register surface with SurfacePlugin'\n/m).test(C.since()),
             'Athens of version 2: ' + JSON.stringify(C.since() + C.errSince()).slice(0, 400));
      C.post({ type: 'save' });
      await waitFor('saved', () => C.saved.length, 60000);
      await C.prompt(60000);
      const m = C.saved[0];
      assert(!m.error && m.webPackage === 2 && m.prepared === false, 'saved ' + JSON.stringify(m));
      const meta = memory.map.get('meta');
      assert(meta.webPackage === 2 && meta.prepared === false, 'meta ' + JSON.stringify(meta));
    } finally { await C.close(); current = S; }
    // the world boot prepares it
    const t = now();
    const P = session(world, { display: true });
    try {
      current = P;
      await waitFor('prepared', () => P.prepared || P.crash || P.exit !== null, 300000);
      P.alive();
      console.log(`#   prepared from version 2 in ${((now() - t) / 1000).toFixed(1)} s`);
      assert(P.ready.source === 'saved' && P.ready.preparing === true && P.ready.prepared === false &&
             P.ready.webPackage === 2, 'ready ' + JSON.stringify(P.ready));
      assert(!P.prepared.error && P.prepared.saved === true && P.prepared.webPackage === webPackage,
             'prepared ' + JSON.stringify(P.prepared));
      assert(P.saved.length === 1 && P.saved[0].prepared === true && P.saved[0].webPackage === webPackage,
             'saved ' + JSON.stringify(P.saved));
      const meta = memory.map.get('meta');
      assert(meta.prepared === true && meta.webPackage === webPackage, 'meta ' + JSON.stringify(meta));
    } finally { await P.close(); current = S; }
    // the extension is back, and Athens and Roassal draw
    const R = session();
    try {
      current = R;
      await R.started();
      assert(R.ready.source === 'saved' && R.ready.webPackage === webPackage && R.ready.prepared === true,
             'ready ' + JSON.stringify(R.ready));
      R.send('OSWebDriver packageVersion\n');
      await R.expectOut(val(String(webPackage)));
      await R.prompt();
      R.send(EXTENSION('/pharo/extension-3.txt'));
      await R.expectOut(/(^|> )[0-9]+\n/m);
      await R.prompt();
      R.post({ type: 'fs', id: 2590, op: 'readFile', path: '/pharo/extension-3.txt' });
      const again = new TextDecoder().decode((await R.reply('fs-result', 2590)).data);
      assert(again === extension, 'the extension after the preparation: ' + JSON.stringify(again));
      R.send(SURFACE);
      await R.prompt();
      assert(val('#drawn').test(R.since()), 'Athens: ' + JSON.stringify(R.since() + R.errSince()).slice(0, 400));
      R.send(ROASSAL);
      await R.prompt(60000);
      assert(val('#(0 0 255)').test(R.since()),
             'the centre of the Roassal canvas: ' + JSON.stringify(R.since() + R.errSince()).slice(0, 400));
    } finally { await R.close(); current = S; }
  });

  // ---- version 3 of OSWindow-Web to 4: Iceberg's https:// remotes
  const iceberg = versions && webPackage >= 4;
  if (!iceberg) console.log('# skip 26: ' + (!versions ? 'no versions (see 20-23)' : 'manifest.webPackage is ' + webPackage));
  if (iceberg) await check('26 an image of version 3, whose Iceberg clones over SSH, is prepared to 4 on the world boot; then its remotes are https:// ones', async () => {
    memory.map.clear();
    // the site's image, made what a site of version 3 saved: the stock
    // remote type of Iceberg (scp-like URLs, SSH, which a browser cannot
    // reach), and version 3
    const C = session();
    try {
      current = C;
      await C.started();
      C.send('Iceberg remoteTypeSelector\n');
      await C.expectOut(val('#httpsUrl'));
      await C.prompt();
      C.send("Iceberg remoteTypeSelector: #scpUrl. OSWebDriver class compile: 'packageVersion ^ 3'; " +
             "compile: 'packageMarker ^ #OSWindowWebPackage3'. { OSWebDriver packageVersion. Iceberg remoteTypeSelector }\n");
      await C.expectOut(val('#(3 #scpUrl)'));
      await C.prompt();
      C.post({ type: 'save' });
      await waitFor('saved', () => C.saved.length, 60000);
      await C.prompt(60000);
      const m = C.saved[0];
      assert(!m.error && m.webPackage === 3 && m.prepared === false, 'saved ' + JSON.stringify(m));
      const meta = memory.map.get('meta');
      assert(meta.webPackage === 3 && meta.prepared === false, 'meta ' + JSON.stringify(meta));
    } finally { await C.close(); current = S; }
    // the world boot prepares it
    const t = now();
    const P = session(world, { display: true });
    try {
      current = P;
      await waitFor('prepared', () => P.prepared || P.crash || P.exit !== null, 300000);
      P.alive();
      console.log(`#   prepared from version 3 in ${((now() - t) / 1000).toFixed(1)} s`);
      assert(P.ready.source === 'saved' && P.ready.preparing === true && P.ready.prepared === false &&
             P.ready.webPackage === 3, 'ready ' + JSON.stringify(P.ready));
      assert(!P.prepared.error && P.prepared.saved === true && P.prepared.webPackage === webPackage,
             'prepared ' + JSON.stringify(P.prepared));
      assert(P.saved.length === 1 && P.saved[0].prepared === true && P.saved[0].webPackage === webPackage,
             'saved ' + JSON.stringify(P.saved));
      const meta = memory.map.get('meta');
      assert(meta.prepared === true && meta.webPackage === webPackage, 'meta ' + JSON.stringify(meta));
    } finally { await P.close(); current = S; }
    // the image has the package of the site, and Iceberg's https:// remotes
    const R = session();
    try {
      current = R;
      await R.started();
      assert(R.ready.source === 'saved' && R.ready.webPackage === webPackage && R.ready.prepared === true,
             'ready ' + JSON.stringify(R.ready));
      R.send('{ OSWebDriver packageVersion. Iceberg remoteTypeSelector }\n');
      await R.expectOut(val(`#(${webPackage} #httpsUrl)`));
      await R.prompt();
    } finally { await R.close(); current = S; }
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
