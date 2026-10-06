// worker-shim.js - run the real vm-worker.js in worker_threads, as a page runs it
//
//   const WorkerShim = require('./lib/worker-shim.js');
//   const shim = await WorkerShim.start(WEB_DIR, { memory, guard });
//
// WEB_DIR is a built web directory (build-wasm/web), served over HTTP by
// packaging/emscripten/tools/serve.mjs as by `make wasm-serve', on a port
// of its own (shim.base).  Every worker runs WEB_DIR/vm-worker.js,
// unmodified, in a worker_threads Worker with a 1 MB stack (about what
// browsers give workers), behind a small shim for the Web Worker globals it
// uses: self, location, importScripts (the files of WEB_DIR, through
// vm.runInThisContext), postMessage and onmessage, fetch (of the server,
// relative to the worker's URL), and a fair MessageChannel.  node has the
// rest (DecompressionStream, Blob, TextDecoder).  The storage of
// vm-storage.js is `memory' (PharoStorage.memory() of the harness), which
// outlives the workers as IndexedDB outlives a page: the shim makes it
// PharoStorage.backend, through a port.  guard(op), when given, is called
// before each operation of that store, and what it throws is the error of
// the operation (a full disk, a database that cannot be opened).
//
//   shim.site({encodeGzip, override})  a URL prefix /s<n>/ of its own:
//       {n, log, url}, where log lists the paths it was asked for (without
//       the query), encodeGzip sends the .gz files with Content-Encoding:
//       gzip, and override {path: contents} answers other contents
//   shim.worker(site, {overrides})  a worker_threads Worker of that site;
//       overrides {name: file} loads another file for an importScripts name
//   shim.webWorker(opts)  a Web Worker as the page has it, on a site of its
//       own (opts are those of site()): {postMessage, terminate, onmessage,
//       onerror, site, diag}, for the createWorker of nb-kernel.js
//
// worker-harness.js and notebook-harness.js use it.

'use strict';
const { Worker, MessageChannel } = require('worker_threads');
const fs = require('fs');
const http = require('http');
const path = require('path');
const { pathToFileURL } = require('url');

const srcDir = path.join(__dirname, '..', '..', '..');

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

async function start(webDir, { memory, guard = null } = {}) {
  webDir = path.resolve(webDir);
  const manifest = JSON.parse(fs.readFileSync(path.join(webDir, 'manifest.json'), 'utf8'));
  const serve = await import(pathToFileURL(path.join(srcDir, 'packaging', 'emscripten', 'tools', 'serve.mjs')));
  const handler = serve.handler(webDir);
  // WEB_DIR under /s<n>/, each with its own log and mode
  const sites = new Map();
  let siteCount = 0;
  const server = http.createServer((req, res) => {
    const m = /^\/s(\d+)(\/.*)$/.exec(req.url);
    const s = m && sites.get(Number(m[1]));
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
    handler(req, res);
  });
  await new Promise(r => server.listen(0, '127.0.0.1', r));
  server.unref();
  const base = 'http://127.0.0.1:' + server.address().port + '/';

  const shim = {
    base, manifest,
    site({ encodeGzip = false, override = null } = {}) {
      const n = ++siteCount;
      const s = { n, log: [], encodeGzip, override, url: base + 's' + n + '/' };
      sites.set(n, s);
      return s;
    },
    worker(site, { overrides = {} } = {}) {
      const store = new MessageChannel();
      store.port1.on('message', async ({ n: id, op, args }) => {
        try {
          if (guard) guard(op);
          store.port1.postMessage({ n: id, value: await memory[op](...args) });
        } catch (e) {
          store.port1.postMessage({ n: id, error: { name: e.name, message: e.message, unavailable: e.unavailable } });
        }
      });
      const w = new Worker(prelude, {
        eval: true, stdout: true, stderr: true,
        workerData: { dir: webDir, url: site.url + 'vm-worker.js?v=' + encodeURIComponent(manifest.build),
                      overrides, storePort: store.port2 },
        transferList: [store.port2],
        resourceLimits: { stackSizeMb: 1 },
      });
      // the port goes with the worker
      w.on('exit', () => store.port1.close());
      return w;
    },
    // A Worker of the page: messages as {data}, an error as {message}
    webWorker(opts = {}) {
      const site = shim.site(opts);
      const w = shim.worker(site, opts);
      const ww = {
        site, diag: '', onmessage: null, onerror: null,
        postMessage: (m, transfer) => w.postMessage(m, transfer),
        terminate: () => { w.terminate(); },
        node: w,
      };
      // what the worker prints (console.warn of the emscripten runtime)
      w.stdout.on('data', d => { ww.diag += d; });
      w.stderr.on('data', d => { ww.diag += d; });
      w.on('message', d => { if (ww.onmessage) ww.onmessage({ data: d }); });
      w.on('error', e => { if (ww.onerror) ww.onerror({ message: String((e && e.message) || e), error: e }); });
      return ww;
    },
    close: () => new Promise(r => server.close(r)),
  };
  return shim;
}

module.exports = { start, prelude };
