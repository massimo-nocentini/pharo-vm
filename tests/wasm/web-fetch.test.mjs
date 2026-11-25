// web-fetch.test.mjs - unit tests of host-events.js, web-fetch.js and node-pump.js
//
// usage: node tests/wasm/web-fetch.test.mjs
//
// Evaluates packaging/emscripten/host/host-events.js and web-fetch.js in fresh
// vm contexts with a fake fetch, and packaging/emscripten/node/node-pump.js
// with a fake module.  Prints every check and exits 1 on failure.

import fs from 'node:fs';
import path from 'node:path';
import vm from 'node:vm';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const hostDir = path.join(here, '..', '..', 'packaging', 'emscripten', 'host');
const hostEventsSrc = fs.readFileSync(path.join(hostDir, 'host-events.js'), 'utf8');
const webFetchSrc = fs.readFileSync(path.join(hostDir, 'web-fetch.js'), 'utf8');
const nodePumpSrc = fs.readFileSync(path.join(hostDir, '..', 'node', 'node-pump.js'), 'utf8');

const dec = new TextDecoder();
const settle = async (turns = 10) => {
  for (let i = 0; i < turns; i++) await new Promise(r => setImmediate(r));
};
const sleep = ms => new Promise(r => setTimeout(r, ms));

let passes = 0;
let failures = 0;

async function check(name, fn) {
  try {
    await fn();
    passes++;
    console.log('ok - ' + name);
  } catch (e) {
    failures++;
    console.log('not ok - ' + name + '\n  ' + String((e && e.stack) || e));
  }
}

function assert(cond, msg) {
  if (!cond) throw new Error(msg || 'assertion failed');
}

function eq(got, want, what) {
  const a = JSON.stringify(got), b = JSON.stringify(want);
  if (a !== b) throw new Error(`${what || 'mismatch'}: got ${a}, want ${b}`);
}

function loadScripts(options = {}) {
  const hasModule = 'Module' in options && options.Module && typeof options.Module === 'object';
  const fetchImpl = 'fetchImpl' in options ? options.fetchImpl : globalThis.fetch;
  const AbortControllerImpl = 'AbortControllerImpl' in options ? options.AbortControllerImpl : AbortController;
  const RequestImpl = 'RequestImpl' in options ? options.RequestImpl : Request;
  const MathObj = 'MathObj' in options ? options.MathObj : Math;
  const setTimeoutFn = 'setTimeoutFn' in options ? options.setTimeoutFn : setTimeout;
  const clearTimeoutFn = 'clearTimeoutFn' in options ? options.clearTimeoutFn : clearTimeout;
  const sandbox = {
    URL,
    TextEncoder,
    TextDecoder,
    Uint8Array,
    Map,
    Set,
    Promise,
    Error,
    TypeError,
    RegExp,
    Math: MathObj,
    String,
    Number,
    Object,
    Array,
    setTimeout: setTimeoutFn,
    clearTimeout: clearTimeoutFn,
    console,
    fetch: fetchImpl,
    AbortController: AbortControllerImpl,
    Request: RequestImpl,
  };
  if (hasModule) sandbox.Module = options.Module;
  if ('processObj' in options) sandbox.process = options.processObj;
  const ctx = vm.createContext(sandbox);
  if (hasModule) vm.runInContext(hostEventsSrc, ctx, { filename: 'host-events.js' });
  vm.runInContext(webFetchSrc, ctx, { filename: 'web-fetch.js' });
  return ctx;
}

function makeFetch({ fetch, isBrowser = false, network = () => true, signals = [], MathObj, setTimeoutFn, clearTimeoutFn } = {}) {
  const opts = { fetchImpl: fetch };
  if (MathObj) opts.MathObj = MathObj;
  if (setTimeoutFn) opts.setTimeoutFn = setTimeoutFn;
  if (clearTimeoutFn) opts.clearTimeoutFn = clearTimeoutFn;
  const ctx = loadScripts(opts);
  const wf = ctx.PharoWebFetch.create({
    signal: i => { signals.push(i); return true; },
    network,
    fetch,
    isBrowser,
  });
  return { wf, signals };
}

function readHead(wf, id) {
  const n = wf.headSize(id);
  if (n < 0) return null;
  const buf = new Uint8Array(n);
  eq(wf.head(id, buf), n, 'head copied count');
  return dec.decode(buf);
}

function makeStream(chunks, { onRead } = {}) {
  let i = 0;
  return {
    getReader() {
      return {
        async read() {
          if (onRead) await onRead(i);
          if (i >= chunks.length) return { done: true, value: undefined };
          const item = chunks[i++];
          if (typeof item === 'function') return await item();
          return { done: false, value: item };
        },
      };
    },
  };
}

function fakeResponse({ status = 200, statusText = 'OK', url = 'http://x.test/', type = 'basic', redirected = false, headers = [], body = null } = {}) {
  return {
    status,
    statusText,
    url,
    type,
    redirected,
    headers: {
      forEach(cb) { for (const [k, v] of headers) cb(v, k); },
    },
    body,
  };
}

// ---------------------------------------------------------------------------
// host-events.js and Module wiring
// ---------------------------------------------------------------------------

await check('host-events: signal, hostWake, late properties, errors, network toggle, shutdown', async () => {
  const signalled = [];
  const errors = [];
  let vmReturn = 1;
  let vmThrow = false;
  const Module = {
    _vm_signal_semaphore(i) {
      if (vmThrow) throw new Error('vm boom');
      signalled.push(i);
      return vmReturn;
    },
    printErr(msg) { errors.push(String(msg)); },
  };
  loadScripts({ Module, fetchImpl: async () => fakeResponse() });

  // hostWake absent is fine
  eq(Module.pharoHost.signal(3), true, 'signal without hostWake');
  eq(signalled, [3], 'semaphore signalled');

  // hostWake and hostNetwork set after load are honoured
  let wakes = 0;
  Module.hostWake = () => { wakes++; };
  eq(Module.pharoHost.signal(7), true, 'signal with late hostWake');
  eq(wakes, 1, 'late hostWake called');

  vmReturn = 0;
  eq(Module.pharoHost.signal(8), false, 'signal returns false when VM returns 0');
  eq(wakes, 1, 'hostWake not called when VM returns 0');
  vmReturn = 1;

  vmThrow = true;
  eq(Module.pharoHost.signal(9), false, 'exception in _vm_signal_semaphore gives false');
  assert(errors.some(m => m.includes('vm boom')), 'signal exception reported to printErr');
  vmThrow = false;

  // Services: an exception in one does not stop the others
  let offs = 0, downs = 0;
  Module.pharoHost.add({
    networkOff() { throw new Error('off boom'); },
    shutdown() { throw new Error('down boom'); },
  });
  Module.pharoHost.add({
    networkOff() { offs++; },
    shutdown() { downs++; },
  });

  // setNetwork(true) when already on calls nothing
  Module.pharoHost.setNetwork(true);
  eq(offs, 0, 'setNetwork(true) does not call networkOff');

  Module.pharoHost.setNetwork(false);
  eq(Module.pharoHost.network, false, 'network off');
  eq(Module.webFetch.availability(), 2, 'availability 2 when off');
  eq(offs, 1, 'second service networkOff ran after first threw');
  assert(errors.some(m => m.includes('off boom')), 'networkOff error reported to printErr');

  Module.pharoHost.setNetwork(true);
  eq(Module.pharoHost.network, true, 'network back on');
  eq(Module.webFetch.availability(), 1, 'availability 1 when back on');

  Module.pharoHost.shutdown();
  Module.pharoHost.shutdown();
  eq(downs, 1, 'second service shutdown ran once after first threw');
  assert(errors.some(m => m.includes('down boom')), 'shutdown error reported to printErr');
  eq(Module.pharoHost.signal(10), false, 'no signal after shutdown');
  Module.pharoHost.add({ networkOff() {}, shutdown() {} });
});

await check('availability with missing fetch/AbortController and hostNetwork before/after load', async () => {
  const ctxNoFetch = loadScripts({ Module: {}, fetchImpl: undefined });
  eq(ctxNoFetch.Module.webFetch.availability(), 0, 'no fetch -> 0');
  eq(ctxNoFetch.Module.webFetch.start(1, 'GET\nhttp://x.test/', null, 1024, 0, 0), 0, 'start refused when 0');

  const ctxNoAbort = loadScripts({ Module: {}, fetchImpl: async () => fakeResponse(), AbortControllerImpl: undefined });
  eq(ctxNoAbort.Module.webFetch.availability(), 0, 'no AbortController -> 0');

  // hostNetwork === false before load
  const mBefore = { hostNetwork: false };
  loadScripts({ Module: mBefore, fetchImpl: async () => fakeResponse() });
  eq(mBefore.webFetch.availability(), 2, 'hostNetwork false before load -> 2');
  mBefore.hostNetwork = true;
  eq(mBefore.webFetch.availability(), 1, 'hostNetwork true after load -> 1');

  // hostNetwork === false set after load
  const mAfter = {};
  loadScripts({ Module: mAfter, fetchImpl: async () => fakeResponse() });
  eq(mAfter.webFetch.availability(), 1, 'default -> 1');
  mAfter.hostNetwork = false;
  eq(mAfter.webFetch.availability(), 2, 'hostNetwork false after load -> 2');
});

await check('Module wiring: webFetch signals through pharoHost and shutdown aborts open fetch', async () => {
  const signalled = [];
  let wakes = 0;
  let aborted = false;
  const Module = {
    _vm_signal_semaphore(i) { signalled.push(i); return 1; },
    hostWake() { wakes++; },
  };
  loadScripts({
    Module,
    fetchImpl: (url, init) => {
      if (url.endsWith('/ok')) return Promise.resolve(fakeResponse({ body: null }));
      return new Promise(() => { init.signal.addEventListener('abort', () => { aborted = true; }); });
    },
  });

  const id1 = Module.webFetch.start(12, 'GET\nhttp://x.test/ok', null, 1024, 0, 0);
  await settle();
  eq(signalled, [12, 12], 'HEAD and DONE signalled through pharoHost');
  eq(wakes, 2, 'hostWake called for each signal');
  Module.webFetch.close(id1);

  const id2 = Module.webFetch.start(13, 'GET\nhttp://x.test/hang', null, 1024, 0, 0);
  await settle();
  Module.pharoHost.shutdown();
  assert(aborted, 'pharoHost.shutdown aborts open fetch');
  eq(Module.webFetch.state(id2), -1, 'forgotten after shutdown');
});

// ---------------------------------------------------------------------------
// States, exact signal counts, unknown id, null body
// ---------------------------------------------------------------------------

await check('states 1 -> 2 -> 3, no sync signal, unknown id, null body', async () => {
  let resolveFetch;
  const signals = [];
  const { wf } = makeFetch({
    signals,
    fetch: () => new Promise(r => { resolveFetch = r; }),
  });

  eq(wf.state(999), -1, 'unknown state');
  eq(wf.headSize(999), -1, 'unknown headSize');
  eq(wf.head(999, new Uint8Array(4)), -1, 'unknown head');
  eq(wf.read(999, new Uint8Array(4)), -1, 'unknown read');
  eq(wf.close(999), 0, 'unknown close');

  const id = wf.start(3, 'GET\nhttp://x.test/null', null, 1024, 0, 0);
  assert(id > 0, 'positive id');
  eq(signals, [], 'start never signals synchronously');
  eq(wf.state(id), 1, 'PENDING with 0 bytes');
  eq(wf.headSize(id), -1, 'headSize -1 in PENDING');
  eq(wf.head(id, new Uint8Array(4)), -1, 'head -1 in PENDING');
  eq(wf.read(id, new Uint8Array(4)), 0, 'read 0 in PENDING');

  await settle();
  resolveFetch(fakeResponse({ status: 204, statusText: 'No Content', url: 'http://x.test/null', headers: [['X-Empty', '']], body: null }));
  await settle();

  eq(wf.state(id), 3, 'DONE with 0 bytes');
  eq(readHead(wf, id), '204\nNo Content\nhttp://x.test/null\nbasic\n0\nX-Empty: ', 'head record');
  eq(signals, [3, 3], 'exact signals: HEAD then DONE');
  eq(wf.close(id), 1, 'close open returns 1');
  eq(wf.close(id), 0, 'second close returns 0');
  eq(wf.state(id), -1, 'unknown after close');
});

await check('exact signal counts across chunks, drains, read(), and failure', async () => {
  const gates = [];
  const pushChunk = () => new Promise(r => { gates.push(r); });
  const signals = [];
  const { wf } = makeFetch({
    signals,
    fetch: async () => fakeResponse({
      body: makeStream([pushChunk, pushChunk, pushChunk, pushChunk]),
    }),
  });

  const id = wf.start(6, 'GET\nhttp://x.test/sig', null, 1024, 0, 0);
  await settle();
  eq(signals, [6], '1 signal on HEAD');

  // Chunk 1: size 0 -> 4 signals
  gates.shift()({ done: false, value: new Uint8Array([1, 2, 3, 4]) });
  await settle();
  eq(signals, [6, 6], '2nd signal on 0 -> 4');

  // Chunk 2 while size > 0: no signal
  gates.shift()({ done: false, value: new Uint8Array([5, 6]) });
  await settle();
  eq(signals, [6, 6], 'no signal when chunk arrives while size > 0');

  // read() itself never signals
  const buf = new Uint8Array(6);
  eq(wf.read(id, buf), 6, 'drained all 6 bytes');
  eq(signals, [6, 6], 'no signal from read()');

  // Chunk 3 after drain to 0: size 0 -> 2 signals
  gates.shift()({ done: false, value: new Uint8Array([7, 8]) });
  await settle();
  eq(signals, [6, 6, 6], '3rd signal on 0 -> 2 after drain');

  // EOF: DONE signals
  gates.shift()({ done: true, value: undefined });
  await settle();
  eq(signals, [6, 6, 6, 6], '4th signal on DONE');
  wf.close(id);
});

// ---------------------------------------------------------------------------
// Window back-pressure and exact-size body
// ---------------------------------------------------------------------------

await check('window back-pressure and exact-size body', async () => {
  let readsIssued = 0;
  const c = n => new Uint8Array(n).fill(65);
  const { wf } = makeFetch({
    fetch: async () => fakeResponse({
      body: makeStream([c(6), c(6), c(6)], { onRead: () => { readsIssued++; } }),
    }),
  });

  // window = 10: reads 6, then 6 (size = 12 >= 10) -> pauses in state 2
  const id = wf.start(5, 'GET\nhttp://x.test/win', null, 10, 0, 0);
  await settle();
  eq(wf.state(id), 2 + 8 * 12, 'paused in HEAD with 12 bytes');
  eq(readsIssued, 2, '3rd read not issued while size >= window');

  // Read 4 -> size = 8 < 10 -> resumes, reads 3rd chunk (6 -> size = 14 >= 10) -> pauses again in state 2
  eq(wf.read(id, new Uint8Array(4)), 4, 'read 4');
  await settle();
  eq(wf.state(id), 2 + 8 * 14, 'still in HEAD with 14 bytes');
  eq(readsIssued, 3, '3rd chunk read, EOF read not issued yet');

  // Read 5 -> size = 9 < 10 -> resumes, reads EOF -> DONE with 9 bytes
  eq(wf.read(id, new Uint8Array(5)), 5, 'read 5');
  await settle();
  eq(wf.state(id), 3 + 8 * 9, 'DONE with 9 bytes');
  eq(readsIssued, 4, 'EOF read issued');
  wf.close(id);

  // Exact-size body with window = max + 1 reaches DONE; max + 1 pauses in HEAD
  const { wf: wfExact } = makeFetch({
    fetch: async () => fakeResponse({ body: makeStream([c(50), c(50)]) }),
  });
  const idExact = wfExact.start(5, 'GET\nhttp://x.test/ex', null, 101, 0, 0);
  await settle();
  eq(wfExact.state(idExact), 3 + 8 * 100, 'exact maxBodySize reaches DONE');
  wfExact.close(idExact);

  const { wf: wfOver } = makeFetch({
    fetch: async () => fakeResponse({ body: makeStream([c(50), c(51), c(1)]) }),
  });
  const idOver = wfOver.start(5, 'GET\nhttp://x.test/over', null, 101, 0, 0);
  await settle();
  eq(wfOver.state(idOver), 2 + 8 * 101, 'max + 1 stays in HEAD');
  wfOver.close(idOver);
});

// ---------------------------------------------------------------------------
// Reads across many chunks (> 1024 chunks)
// ---------------------------------------------------------------------------

await check('reads across 1200 chunks (partial, boundary, compaction)', async () => {
  const chunks = [];
  const expected = [];
  for (let i = 0; i < 1200; i++) {
    const ch = new Uint8Array([i & 255, (i >> 8) & 255, 0x5a]);
    chunks.push(ch);
    expected.push(...ch);
  }
  const { wf } = makeFetch({
    fetch: async () => fakeResponse({ body: makeStream(chunks) }),
  });
  const id = wf.start(2, 'GET\nhttp://x.test/chunks', null, 10000, 0, 0);
  await settle(20);
  eq(wf.state(id), 3 + 8 * 3600, 'all 3600 bytes ready');

  const got = [];
  const dest = new Uint8Array(7);
  for (;;) {
    const n = wf.read(id, dest);
    if (n === 0) break;
    for (let i = 0; i < n; i++) got.push(dest[i]);
  }
  eq(got.length, expected.length, 'total length');
  for (let i = 0; i < expected.length; i++) {
    if (got[i] !== expected[i]) throw new Error(`byte mismatch at ${i}: ${got[i]} vs ${expected[i]}`);
  }
  eq(wf.state(id), 3, 'DONE with 0 bytes left');
  wf.close(id);
});

// ---------------------------------------------------------------------------
// Timeouts (in state 1, in state 2, and cleared at DONE and close)
// ---------------------------------------------------------------------------

await check('timeout in PENDING, in HEAD (drops bytes), and cleared at DONE and close', async () => {
  // Timeout in PENDING
  let aborted = false;
  const signals = [];
  const { wf } = makeFetch({
    signals,
    fetch: (url, init) => {
      if (url.endsWith('/fast')) return Promise.resolve(fakeResponse({ body: null }));
      if (url.endsWith('/head-stall')) {
        return Promise.resolve(fakeResponse({
          body: makeStream([new Uint8Array([1, 2, 3, 4]), () => new Promise(() => {})]),
        }));
      }
      return new Promise((res, rej) => {
        init.signal.addEventListener('abort', () => {
          aborted = true;
          const e = new Error('aborted');
          e.name = 'AbortError';
          rej(e);
        });
      });
    },
  });

  const id1 = wf.start(9, 'GET\nhttp://x.test/slow', null, 1024, 25, 0);
  await sleep(60);
  assert(aborted, 'AbortController signalled on timeout');
  eq(wf.state(id1), 4, 'FAILED with 0 bytes');
  assert(readHead(wf, id1).startsWith('timeout\n'), 'timeout kind');
  eq(wf.read(id1, new Uint8Array(8)), 0, 'read 0 in FAILED');
  const sigCount = signals.length;
  wf.close(id1);
  await settle();
  eq(signals.length, sigCount, 'no signal after close');

  // Timeout in state 2 (HEAD with buffered bytes) drops bytes and sets kind timeout
  const id2 = wf.start(9, 'GET\nhttp://x.test/head-stall', null, 1024, 25, 0);
  await settle();
  eq(wf.state(id2), 2 + 8 * 4, '4 bytes buffered in HEAD');
  await sleep(60);
  eq(wf.state(id2), 4, 'state 4 with 0 bytes after timeout in HEAD');
  assert(readHead(wf, id2).startsWith('timeout\n'), 'timeout kind in HEAD');
  wf.close(id2);

  // Timer cleared at DONE: later tick changes nothing
  const id3 = wf.start(9, 'GET\nhttp://x.test/fast', null, 1024, 25, 0);
  await settle();
  eq(wf.state(id3), 3, 'DONE immediately');
  await sleep(60);
  eq(wf.state(id3), 3, 'still DONE after timeout duration');
  assert(readHead(wf, id3).startsWith('200\n'), 'head unchanged');
  wf.close(id3);

  // Timer cleared at close: no signal after timeout duration
  const id4 = wf.start(9, 'GET\nhttp://x.test/hang', null, 1024, 25, 0);
  await settle();
  const sigBefore = signals.length;
  wf.close(id4);
  await sleep(60);
  eq(signals.length, sigBefore, 'no signal from cleared timer after close');
});

// ---------------------------------------------------------------------------
// Close while reader.read() is pending and while paused on window
// ---------------------------------------------------------------------------

await check('close with read pending and while paused on window: no further read or signal', async () => {
  // Pending reader.read()
  let aborted = false;
  let resolveRead;
  const signals = [];
  const { wf } = makeFetch({
    signals,
    fetch: async (url, init) => {
      init.signal.addEventListener('abort', () => { aborted = true; });
      return fakeResponse({
        body: makeStream([() => new Promise(r => { resolveRead = r; })]),
      });
    },
  });
  const id = wf.start(11, 'GET\nhttp://x.test/pending-read', null, 1024, 0, 0);
  await settle();
  eq(wf.state(id) & 7, 2, 'in HEAD');
  const beforeClose = signals.length;
  eq(wf.close(id), 1, 'close returns 1');
  assert(aborted, 'close aborts in-flight request');
  resolveRead({ done: false, value: new Uint8Array([1, 2, 3]) });
  await settle();
  eq(signals.length, beforeClose, 'no signal after close');

  // Paused on window
  let readsIssued = 0;
  const signals2 = [];
  const { wf: wf2 } = makeFetch({
    signals: signals2,
    fetch: async () => fakeResponse({
      body: makeStream([new Uint8Array(10), new Uint8Array(10)], { onRead: () => { readsIssued++; } }),
    }),
  });
  const id2 = wf2.start(12, 'GET\nhttp://x.test/win-close', null, 8, 0, 0);
  await settle();
  eq(wf2.state(id2), 2 + 8 * 10, 'paused at window');
  eq(readsIssued, 1, '1 read issued');
  const beforeClose2 = signals2.length;
  eq(wf2.close(id2), 1, 'close while paused on window');
  await settle();
  eq(readsIssued, 1, 'reader.read not called again after close');
  eq(signals2.length, beforeClose2, 'no signal after close on window');
});

// ---------------------------------------------------------------------------
// networkOff and shutdown
// ---------------------------------------------------------------------------

await check('networkOff: DONE intact, state 2 becomes state 4 (0 bytes, loop stops); shutdown forgets all', async () => {
  let netOn = true;
  let aborts = 0;
  let winReads = 0;
  const { wf } = makeFetch({
    network: () => netOn,
    fetch: (url, init) => {
      init.signal.addEventListener('abort', () => { aborts++; });
      if (url.endsWith('/done')) {
        return Promise.resolve(fakeResponse({ body: makeStream([new Uint8Array([9, 8, 7])]) }));
      }
      if (url.endsWith('/win')) {
        return Promise.resolve(fakeResponse({
          body: makeStream([new Uint8Array(10), new Uint8Array(10)], { onRead: () => { winReads++; } }),
        }));
      }
      return new Promise(() => {});
    },
  });

  const idDone = wf.start(4, 'GET\nhttp://x.test/done', null, 1024, 0, 0);
  const idWin = wf.start(4, 'GET\nhttp://x.test/win', null, 8, 0, 0);
  const idPend = wf.start(4, 'GET\nhttp://x.test/pend', null, 1024, 0, 0);
  await settle();

  eq(wf.state(idDone), 3 + 8 * 3, 'idDone in state 3');
  eq(wf.state(idWin), 2 + 8 * 10, 'idWin paused in state 2');
  eq(winReads, 1, '1 read on idWin');

  netOn = false;
  wf.networkOff();
  await settle();

  // DONE stays intact and readable
  eq(wf.state(idDone), 3 + 8 * 3, 'DONE unchanged by networkOff');
  const got = new Uint8Array(3);
  eq(wf.read(idDone, got), 3, 'DONE bytes still readable');
  eq([...got], [9, 8, 7], 'DONE bytes match');

  // State 2 with buffered bytes becomes state 4 with 0 bytes and stops reading
  eq(wf.state(idWin), 4, 'idWin FAILED with 0 bytes');
  assert(readHead(wf, idWin).startsWith('off\n'), 'idWin kind off');
  eq(winReads, 1, 'reader.read not called again after networkOff');

  // PENDING becomes state 4 with kind off
  eq(wf.state(idPend), 4, 'idPend FAILED');
  assert(readHead(wf, idPend).startsWith('off\n'), 'idPend kind off');
  eq(aborts, 2, 'only non-DONE requests aborted');
  eq(wf.start(4, 'GET\nhttp://x.test/c', null, 1024, 0, 0), 0, 'start refused while off');

  wf.shutdown();
  eq(wf.state(idDone), -1, 'forgotten after shutdown');
  eq(wf.close(idPend), 0, 'close 0 after shutdown');
});

// ---------------------------------------------------------------------------
// Header filtering, request body, exact init keys
// ---------------------------------------------------------------------------

await check('header filtering (browser vs node), request body, and exact init keys', async () => {
  const seen = [];
  const fake = async (url, init) => { seen.push({ url, init }); return fakeResponse(); };
  const allHeaders = [
    'Accept-Charset: utf-8',
    'Accept-Encoding: gzip',
    'Access-Control-Request-Headers: x',
    'Access-Control-Request-Method: POST',
    'Connection: keep-alive',
    'Content-Length: 5',
    'Cookie: a=1',
    'Cookie2: b=2',
    'Date: Wed, 01 Jan 2025 00:00:00 GMT',
    'DNT: 1',
    'Expect: 100-continue',
    'Host: evil.test',
    'Keep-Alive: timeout=5',
    'Origin: http://other.test',
    'Referer: http://ref.test',
    'Set-Cookie: c=3',
    'TE: trailers',
    'Trailer: Expires',
    'Transfer-Encoding: chunked',
    'Upgrade: websocket',
    'Via: 1.1 proxy',
    'Proxy-Authorization: secret',
    'Sec-Fetch-Mode: cors',
    'User-Agent: Pharo/12',
    'X-Custom: ok',
    'Authorization: Bearer tok',
  ];
  const reqText = 'post\nhttp://x.test/h\n' + allHeaders.join('\n');
  const payload = new Uint8Array([10, 20, 30]);

  const { wf: wfBrowser } = makeFetch({ fetch: fake, isBrowser: true });
  const idB = wfBrowser.start(1, reqText, payload, 1024, 0, 0);
  await settle();
  wfBrowser.close(idB);

  const { wf: wfNode } = makeFetch({ fetch: fake, isBrowser: false });
  const idN = wfNode.start(1, reqText, payload, 1024, 0, 0);
  const idNull = wfNode.start(1, 'GET\nhttp://x.test/g', null, 1024, 0, 0);
  await settle();
  wfNode.close(idN);
  wfNode.close(idNull);

  eq(seen.length, 3, 'three fetches');
  eq(Object.keys(seen[0].init).sort(),
    ['body', 'cache', 'credentials', 'headers', 'method', 'mode', 'redirect', 'referrerPolicy', 'signal'],
    'exact init keys');
  eq(seen[0].init.method, 'POST', 'method upper-cased');
  eq([...seen[0].init.body], [10, 20, 30], 'body bytes passed through');
  eq(seen[2].init.body, undefined, 'null body gives undefined');
  eq(seen[0].init.mode, 'cors', 'mode cors');
  eq(seen[0].init.credentials, 'omit', 'credentials omit');
  eq(seen[0].init.referrerPolicy, 'no-referrer', 'no-referrer');
  eq(seen[0].init.cache, 'default', 'cache default');

  eq(seen[0].init.headers, [['X-Custom', 'ok'], ['Authorization', 'Bearer tok']], 'browser drops User-Agent and forbidden');
  eq(seen[1].init.headers, [['User-Agent', 'Pharo/12'], ['X-Custom', 'ok'], ['Authorization', 'Bearer tok']], 'node keeps User-Agent, drops forbidden');
});

// ---------------------------------------------------------------------------
// Redirect modes, records (UTF-8, empty reason, redirected, NUL), error kinds
// ---------------------------------------------------------------------------

await check('redirect modes, record encoding/sanitization, and network/body/aborted error kinds', async () => {
  const seenRedirect = [];
  const { wf } = makeFetch({
    fetch: async (url, init) => {
      seenRedirect.push(init.redirect);
      if (url.endsWith('/opaque')) return fakeResponse({ type: 'opaqueredirect', status: 0 });
      if (url.endsWith('/302')) return fakeResponse({
        status: 302, statusText: 'Found\r\nEvil\0', url: 'http://x.test/302\n',
        type: 'cors', redirected: true, headers: [['Location', '/ok\r\nX-Injected: 1'], ['X-Nul', 'a\0b']],
      });
      if (url.endsWith('/utf8')) return fakeResponse({
        status: 200, statusText: 'Caf\u00e9', url: 'http://x.test/utf8', type: 'cors', redirected: true,
      });
      if (url.endsWith('/empty-reason')) return fakeResponse({
        status: 200, statusText: '', url: 'http://x.test/empty-reason', type: 'basic', redirected: false,
      });
      if (url.endsWith('/neterr')) {
        const err = new TypeError('fetch failed');
        err.cause = { code: 'ECONNREFUSED', message: 'connect\r\nrefused\0' };
        throw err;
      }
      if (url.endsWith('/bodyerr')) {
        return fakeResponse({ body: makeStream([() => Promise.reject(new Error('stream\nbroken'))]) });
      }
      if (url.endsWith('/external-abort')) {
        const e = new Error('external cancel');
        e.name = 'AbortError';
        throw e;
      }
      return fakeResponse();
    },
  });

  const id0 = wf.start(1, 'GET\nhttp://x.test/ok', null, 1024, 0, 0);
  const id1 = wf.start(1, 'GET\nhttp://x.test/opaque', null, 1024, 0, 1);
  const id2 = wf.start(1, 'GET\nhttp://x.test/302', null, 1024, 0, 1);
  const id3 = wf.start(1, 'GET\nhttp://x.test/utf8', null, 1024, 0, 0);
  const id4 = wf.start(1, 'GET\nhttp://x.test/empty-reason', null, 1024, 0, 0);
  const id5 = wf.start(1, 'GET\nhttp://x.test/neterr', null, 1024, 0, 0);
  const id6 = wf.start(1, 'GET\nhttp://x.test/bodyerr', null, 1024, 0, 0);
  const id7 = wf.start(1, 'GET\nhttp://x.test/external-abort', null, 1024, 0, 0);
  await settle();

  eq(wf.state(id1), 4, 'opaque redirect fails');
  assert(readHead(wf, id1).startsWith('redirect\n'), 'kind redirect');

  eq(wf.state(id2), 3, '302 succeeds in manual mode');
  const h2 = readHead(wf, id2);
  assert(!h2.includes('\r') && !h2.includes('\0'), 'no CR or NUL in head');
  eq(h2.split('\n').length, 7, 'sanitized fields did not inject lines');
  assert(h2.includes('X-Nul: a b'), 'NUL in header value replaced by space');

  eq(readHead(wf, id3), '200\nCaf\u00e9\nhttp://x.test/utf8\ncors\n1', 'UTF-8 reason, cors type, redirected 1');
  eq(readHead(wf, id4), '200\n\nhttp://x.test/empty-reason\nbasic\n0', 'empty statusText');

  const netRec = readHead(wf, id5);
  assert(netRec.startsWith('network\n') && netRec.includes('ECONNREFUSED'), 'network kind with cause.code');
  assert(!netRec.includes('\r') && !netRec.includes('\0') && netRec.split('\n').length === 2, 'error sanitized');

  assert(readHead(wf, id6).startsWith('body\n'), 'body kind');
  assert(readHead(wf, id7).startsWith('aborted\n'), 'external AbortError gives kind aborted');

  eq(seenRedirect, ['follow', 'manual', 'manual', 'follow', 'follow', 'follow', 'follow', 'follow'], 'redirect modes');
  for (const id of [id0, id1, id2, id3, id4, id5, id6, id7]) wf.close(id);
});

// ---------------------------------------------------------------------------
// Unique ids, wrap from 0x7fffffff to 1, 65th start returns 0
// ---------------------------------------------------------------------------

await check('id wrap from 0x7fffffff to 1, unique ids, and 65th start returns 0', async () => {
  // Math.random() just under 1 starts counter at 0x7ffffffe -> next ids are 0x7fffffff, 1, 2
  const { wf: wfWrap } = makeFetch({
    fetch: () => new Promise(() => {}),
    MathObj: { floor: Math.floor, random: () => 0.9999999999 },
  });
  const w1 = wfWrap.start(1, 'GET\nhttp://x.test/w1', null, 1024, 0, 0);
  const w2 = wfWrap.start(1, 'GET\nhttp://x.test/w2', null, 1024, 0, 0);
  const w3 = wfWrap.start(1, 'GET\nhttp://x.test/w3', null, 1024, 0, 0);
  eq([w1, w2, w3], [0x7fffffff, 1, 2], 'wraps from 0x7fffffff to 1');
  wfWrap.close(w1); wfWrap.close(w2); wfWrap.close(w3);

  const { wf } = makeFetch({ fetch: () => new Promise(() => {}) });
  const ids = new Set();
  for (let i = 0; i < 64; i++) {
    const id = wf.start(1, 'GET\nhttp://x.test/' + i, null, 1024, 0, 0);
    assert(id >= 1 && id <= 0x7fffffff, '31-bit positive id: ' + id);
    assert(!ids.has(id), 'unique id');
    ids.add(id);
  }
  eq(wf.start(1, 'GET\nhttp://x.test/65', null, 1024, 0, 0), 0, '65th start returns 0');

  const first = [...ids][0];
  eq(wf.close(first), 1, 'close one');
  const next = wf.start(1, 'GET\nhttp://x.test/66', null, 1024, 0, 0);
  assert(next >= 1 && !ids.has(next), '66th succeeds with fresh id');
  ids.delete(first);
  ids.add(next);
  for (const id of ids) wf.close(id);
});

// ---------------------------------------------------------------------------
// URL and request validation refusals (kind 'request', fetch never called)
// ---------------------------------------------------------------------------

await check('ftp:/data:/blob:/user info/relative/bad header/GET+body refused as request', async () => {
  let calls = 0;
  const { wf } = makeFetch({ fetch: async () => { calls++; return fakeResponse(); } });
  const bad = [
    ['GET\nftp://example.com/a', null],
    ['GET\ndata:text/plain,hello', null],
    ['GET\nblob:https://example.com/uuid', null],
    ['GET\nhttp://user:pass@example.com/', null],
    ['GET\nhttp://user@example.com/', null],
    ['GET\nhttp://:pass@example.com/', null],
    ['GET\n/relative/path', null],
    ['GET\nhttp://example.com/\nBadHeaderNoColon', null],
    ['GET\nhttp://example.com/', new Uint8Array([1])],
  ];
  for (const [text, body] of bad) {
    const id = wf.start(1, text, body, 1024, 0, 0);
    assert(id > 0, 'start returns id before async validation');
    await settle();
    eq(wf.state(id), 4, 'FAILED for ' + text);
    assert(readHead(wf, id).startsWith('request\n'), 'kind request for ' + text);
    wf.close(id);
  }
  eq(calls, 0, 'fetch never invoked for invalid requests');
});

// ---------------------------------------------------------------------------
// Async start boundary, empty chunks, late chunks/responses, timer cleanup
// ---------------------------------------------------------------------------

await check('start() is strictly async: no sync signal on bad request, no sync fetch on good request', async () => {
  let fetchCalls = 0;
  const signals = [];
  const { wf } = makeFetch({
    signals,
    fetch: async () => { fetchCalls++; return fakeResponse(); },
  });

  const idBad = wf.start(1, 'GET\nftp://example.com/x', null, 1024, 0, 0);
  eq(signals, [], 'refused request does not signal synchronously');
  eq(wf.state(idBad), 1, 'refused request still PENDING synchronously');
  eq(wf.headSize(idBad), -1, 'no head synchronously');

  const idGood = wf.start(2, 'GET\nhttp://x.test/ok', null, 1024, 0, 0);
  eq(fetchCalls, 0, 'fetch not called synchronously inside start');
  eq(signals, [], 'good request does not signal synchronously');

  await settle();
  eq(fetchCalls, 1, 'fetch called asynchronously');
  eq(wf.state(idBad), 4, 'refused request failed asynchronously');
  wf.close(idBad);
  wf.close(idGood);
});

await check('empty chunk at size 0 does not signal and is skipped on read', async () => {
  const gates = [];
  const next = () => new Promise(r => { gates.push(r); });
  const signals = [];
  const { wf } = makeFetch({
    signals,
    fetch: async () => fakeResponse({ body: makeStream([next, next, next]) }),
  });

  const id = wf.start(6, 'GET\nhttp://x.test/empty-chunk', null, 1024, 0, 0);
  await settle();
  eq(signals, [6], 'HEAD signal');

  gates.shift()({ done: false, value: new Uint8Array(0) });
  await settle();
  eq(signals, [6], 'empty chunk at size 0 must not signal');
  eq(wf.state(id), 2, 'size still 0 after empty chunk');

  gates.shift()({ done: false, value: new Uint8Array([42, 43]) });
  await settle();
  eq(signals, [6, 6], 'non-empty chunk signals');
  const dest = new Uint8Array(4);
  eq(wf.read(id, dest), 2, 'read gets bytes of next chunk');
  eq([...dest.subarray(0, 2)], [42, 43], 'bytes match');

  gates.shift()({ done: true, value: undefined });
  await settle();
  wf.close(id);
});

await check('chunk arriving after networkOff while read pending leaves state 4 (0 bytes) and no signal', async () => {
  let resolveRead;
  const signals = [];
  const { wf } = makeFetch({
    signals,
    fetch: async () => fakeResponse({
      body: makeStream([() => new Promise(r => { resolveRead = r; })]),
    }),
  });

  const id = wf.start(7, 'GET\nhttp://x.test/late-chunk', null, 1024, 0, 0);
  await settle();
  wf.networkOff();
  eq(wf.state(id), 4, 'FAILED with 0 bytes after networkOff');
  const sigCount = signals.length;

  resolveRead({ done: false, value: new Uint8Array([1, 2, 3]) });
  await settle();
  eq(wf.state(id), 4, 'state stays exactly 4 with 0 bytes');
  eq(signals.length, sigCount, 'no signal for late chunk');
  wf.close(id);
});

await check('timer handles tracked: none at timeout 0, cleared at DONE, close, and fail', async () => {
  const active = new Map();
  let nextHandle = 100;
  let setCount = 0;
  const setTimeoutFn = (fn, ms) => {
    const h = nextHandle++;
    setCount++;
    active.set(h, fn);
    return h;
  };
  const clearTimeoutFn = h => { active.delete(h); };

  const { wf } = makeFetch({
    setTimeoutFn,
    clearTimeoutFn,
    fetch: url => {
      if (url.endsWith('/ok')) return Promise.resolve(fakeResponse({ body: null }));
      if (url.endsWith('/fail')) return Promise.reject(new Error('net down'));
      return new Promise(() => {});
    },
  });

  // timeout 0 sets no timer
  const id0 = wf.start(1, 'GET\nhttp://x.test/ok', null, 1024, 0, 0);
  await settle();
  eq(setCount, 0, 'timeout 0 sets no timer');
  wf.close(id0);

  // DONE clears timer
  const idDone = wf.start(1, 'GET\nhttp://x.test/ok', null, 1024, 50, 0);
  eq(active.size, 1, 'timer active on start');
  await settle();
  eq(active.size, 0, 'timer cleared at DONE');
  wf.close(idDone);

  // close clears timer
  const idClose = wf.start(1, 'GET\nhttp://x.test/hang', null, 1024, 50, 0);
  eq(active.size, 1, 'timer active before close');
  wf.close(idClose);
  eq(active.size, 0, 'timer cleared at close');

  // fail clears timer
  const idFail = wf.start(1, 'GET\nhttp://x.test/fail', null, 1024, 50, 0);
  eq(active.size, 1, 'timer active before fail');
  await settle();
  eq(wf.state(idFail), 4, 'request failed');
  eq(active.size, 0, 'timer cleared at fail');
  wf.close(idFail);
});

await check('fetch ignoring abort resolving after close or timeout does not touch body or overwrite state', async () => {
  let resolveA, resolveB;
  let readerCalls = 0;
  const signals = [];
  const active = new Map();
  let nextHandle = 1;
  const { wf } = makeFetch({
    signals,
    setTimeoutFn: fn => { const h = nextHandle++; active.set(h, fn); return h; },
    clearTimeoutFn: h => { active.delete(h); },
    fetch: url => new Promise(r => {
      if (url.endsWith('/a')) resolveA = r;
      else resolveB = r;
    }),
  });

  const bodySpy = { getReader() { readerCalls++; return makeStream([]).getReader(); } };

  // Case A: resolves after close
  const idA = wf.start(8, 'GET\nhttp://x.test/a', null, 1024, 0, 0);
  await settle();
  const sigBeforeClose = signals.length;
  wf.close(idA);
  resolveA(fakeResponse({ body: bodySpy }));
  await settle();
  eq(readerCalls, 0, 'getReader not called after close');
  eq(signals.length, sigBeforeClose, 'no signal after close');

  // Case B: resolves after timeout
  const idB = wf.start(9, 'GET\nhttp://x.test/b', null, 1024, 50, 0);
  await settle();
  // Fire the timeout callback directly
  for (const fn of [...active.values()]) fn();
  eq(wf.state(idB), 4, 'FAILED on timeout');
  const sigAfterTimeout = signals.length;
  resolveB(fakeResponse({ status: 200, body: bodySpy }));
  await settle();
  eq(readerCalls, 0, 'getReader not called after timeout');
  eq(wf.state(idB), 4, 'state remains 4 after late fetch resolve');
  assert(readHead(wf, idB).startsWith('timeout\n'), 'timeout head preserved');
  eq(signals.length, sigAfterTimeout, 'no signal after late fetch resolve');
  wf.close(idB);
});

await check('signal when size reaches window across multiple chunks, then drain to DONE', async () => {
  const c4 = () => new Uint8Array([1, 2, 3, 4]);
  const signals = [];
  const { wf } = makeFetch({
    signals,
    fetch: async () => fakeResponse({ body: makeStream([c4(), c4(), c4(), c4()]) }),
  });

  const id = wf.start(7, 'GET\nhttp://x.test/win-sig', null, 10, 0, 0);
  await settle();
  // HEAD, 0 -> 4, and 8 -> 12 (>= window 10); 4 -> 8 does not signal
  eq(signals, [7, 7, 7], 'exact 3 signals: HEAD, first bytes, window reached');
  eq(wf.state(id), 2 + 8 * 12, 'state 2 with 12 bytes');

  eq(wf.read(id, new Uint8Array(12)), 12, 'drain all 12 bytes');
  await settle();
  // 0 -> 4, then DONE
  eq(signals, [7, 7, 7, 7, 7], 'exact 5 signals after drain and DONE');
  eq(wf.state(id), 3 + 8 * 4, 'state 3 with 4 bytes');
  wf.close(id);
});

await check('start followed immediately by close never calls fetch', async () => {
  let fetchCalls = 0;
  const signals = [];
  const { wf } = makeFetch({
    signals,
    fetch: async () => { fetchCalls++; return fakeResponse(); },
  });

  const id = wf.start(3, 'GET\nhttp://x.test/immediate-close', null, 1024, 0, 0);
  eq(wf.close(id), 1, 'close immediately after start');
  await settle();
  eq(fetchCalls, 0, 'fetch never called');
  eq(signals, [], 'no signal');
  eq(wf.state(id), -1, 'unknown after close');
});

// node-pump.js on a Module whose first slice wakes it and then ends the VM
for (const [how, last] of [
  ['an ExitStatus', () => { throw Object.assign(new Error('exit'), { name: 'ExitStatus' }); }],
  ['a trap', () => { throw new Error('trap'); }],
  ['another state', () => 9],
]) {
  await check('node-pump.js: no slice after ' + how, async () => {
    const done = { slices: 0, shutdowns: 0 };
    const Module = {
      pharoHost: { shutdown() { done.shutdowns++; } },
      _vm_wakeup_ms: () => 1,
      _vm_resume() {
        done.slices++;
        Module.hostWake();
        return last();
      },
    };
    const ctx = vm.createContext({
      Module, setImmediate, clearImmediate, setTimeout, clearTimeout,
      process: { env: {}, stderr: { write() {} } },
    });
    vm.runInContext(nodePumpSrc, ctx, { filename: 'node-pump.js' });
    Module.onVMStarted();
    await new Promise(resolve => setTimeout(resolve, 20));
    eq(done, { slices: 1, shutdowns: 1 });
    Module.hostWake();
    await new Promise(resolve => setTimeout(resolve, 20));
    eq(done.slices, 1, 'slices after a late hostWake');
  });
}

console.log(`${passes} passed, ${failures} failed`);
if (failures > 0) process.exit(1);
