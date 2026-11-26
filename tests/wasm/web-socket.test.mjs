// web-socket.test.mjs - unit tests of web-socket.js, the WebSocket service of the VMs, and of the exit guard of node-pump.js
//
// usage: node tests/wasm/web-socket.test.mjs
//
// Evaluates packaging/emscripten/host/web-socket.js (with host-events.js
// where Module.pharoHost is needed) in fresh vm contexts, with a
// FakeWebSocket (it records the arguments of its constructor, binaryType at
// the first event, its sends and closes; readyState and bufferedAmount are
// set by the test; fire(type, init) delivers an event), fake timers (advanced
// by hand, the pending ones counted) and a signal that records the indexes;
// and packaging/emscripten/node/node-pump.js with a fake module and a fake
// process.  Prints every check and exits 1 on failure.

import fs from 'node:fs';
import path from 'node:path';
import vm from 'node:vm';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const hostDir = path.join(here, '..', '..', 'packaging', 'emscripten', 'host');
const hostEventsSrc = fs.readFileSync(path.join(hostDir, 'host-events.js'), 'utf8');
const webSocketSrc = fs.readFileSync(path.join(hostDir, 'web-socket.js'), 'utf8');
const nodePumpSrc = fs.readFileSync(path.join(hostDir, '..', 'node', 'node-pump.js'), 'utf8');

const enc = new TextEncoder(), dec = new TextDecoder();
const MiB = 1024 * 1024;
const settle = async (turns = 5) => {
  for (let i = 0; i < turns; i++) await new Promise(r => setImmediate(r));
};

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

// ---- the fakes

function fakeTimers() {
  let now = 0, n = 0;
  const pending = new Map();
  const t = {
    made: 0,
    unrefs: 0,
    setTimeout(f, ms) {
      const h = { n: ++n, at: now + ms, f, unref() { t.unrefs++; } };
      pending.set(h.n, h);
      t.made++;
      return h;
    },
    clearTimeout(h) { if (h) pending.delete(h.n); },
    get count() { return pending.size; },
    get now() { return now; },
    delays() { return [...pending.values()].map(h => h.at - now).sort((a, b) => a - b); },
    // runs the timers due within ms, in order, with the microtasks after each
    async advance(ms) {
      const end = now + ms;
      for (;;) {
        let next = null;
        for (const h of pending.values()) if (h.at <= end && (!next || h.at < next.at)) next = h;
        if (!next) break;
        pending.delete(next.n);
        now = next.at;
        next.f();
        await settle(1);
      }
      now = end;
    },
  };
  return t;
}

function fakeWebSocketClass() {
  class FakeWebSocket {
    constructor(url, protocols) {
      if (FakeWebSocket.throwNext) {
        const e = FakeWebSocket.throwNext;
        FakeWebSocket.throwNext = null;
        throw e;
      }
      this.url = url;
      this.protocols = protocols;
      this.readyState = 0;
      this.bufferedAmount = 0;
      this.protocol = '';
      this.extensions = '';
      this.binaryType = 'blob';
      this.binaryTypeAtFirstEvent = null;
      this.sent = [];
      this.closes = [];
      this.grows = true;          // bufferedAmount grows by what is sent
      this.listeners = {};
      FakeWebSocket.instances.push(this);
    }
    addEventListener(type, f) { (this.listeners[type] = this.listeners[type] || []).push(f); }
    send(data) {
      if (this.readyState === 0) throw Object.assign(new Error('still connecting'), { name: 'InvalidStateError' });
      this.sent.push(data);
      if (this.grows && this.readyState === 1) this.bufferedAmount += typeof data === 'string' ? enc.encode(data).length : data.byteLength;
    }
    close(code, reason) {
      this.closes.push(code === undefined ? [] : [code, reason]);
      // node 24 and 25: a close while connecting fires error and close
      // inside it, then leaves readyState at 2 (CLOSING), not 3
      if (FakeWebSocket.nodeLike && this.readyState === 0) {
        this.fire('error');
        this.fire('close', { code: 1006, reason: '', wasClean: false });
        this.readyState = 2;
        return;
      }
      if (this.readyState < 2) this.readyState = 2;
    }
    fire(type, init = {}) {
      if (this.binaryTypeAtFirstEvent === null) this.binaryTypeAtFirstEvent = this.binaryType;
      if (type === 'open') this.readyState = 1;
      if (type === 'close') this.readyState = 3;
      for (const f of this.listeners[type] || []) f(Object.assign({ type }, init));
    }
  }
  FakeWebSocket.instances = [];
  FakeWebSocket.throwNext = null;
  FakeWebSocket.nodeLike = false;
  return FakeWebSocket;
}

function context(extra = {}) {
  const sandbox = Object.assign({
    URL, TextEncoder, TextDecoder, Uint8Array, ArrayBuffer, Map, Set, Promise, Error, TypeError, RegExp, Math, String,
    Number, Object, Array, JSON, setTimeout, clearTimeout, console,
  }, extra);
  return vm.createContext(sandbox);
}

// A service on fakes: {ws (the service), signals, timers, WS (the class), last()}
function make({ network = () => true, isBrowser = false, siteHostname = '', withWebSocket = true, Math: MathObj, maxQueuedTotal } = {}) {
  const ctx = context(MathObj ? { Math: MathObj } : {});
  vm.runInContext(webSocketSrc, ctx, { filename: 'web-socket.js' });
  const WS = fakeWebSocketClass(), timers = fakeTimers(), signals = [];
  const options = {
    signal: i => { signals.push(i); return true; },
    network,
    WebSocket: () => withWebSocket ? WS : undefined,
    isBrowser, siteHostname,
    setTimeout: (f, ms) => timers.setTimeout(f, ms),
    clearTimeout: h => timers.clearTimeout(h),
  };
  if (maxQueuedTotal) options.maxQueuedTotal = maxQueuedTotal;
  const ws = ctx.PharoWebSocket.create(options);
  return { ws, signals, timers, WS, last: () => WS.instances[WS.instances.length - 1], ctx };
}

const rec = (url, ...protocols) => enc.encode([url, ...protocols].join('\n'));
const URL1 = 'ws://example.test/chat';
// open with the semaphores 11, 12, 13 (state, read, write) unless given
const openOn = (s, url = URL1, { sems = [11, 12, 13], maxMessage = 1000, maxQueued = 10000, timeout = 0, protocols = [] } = {}) =>
  s.ws.open(...sems, rec(url, ...protocols), maxMessage, maxQueued, timeout);
const info = (ws, id) => {
  const n = ws.infoSize(id);
  if (n < 0) return null;
  const b = new Uint8Array(n);
  eq(ws.info(id, b), n, 'info copied');
  return dec.decode(b).split('\n');
};
const take = (ws, id) => {
  const p = ws.peek(id);
  if (p < 0) return null;
  const b = new Uint8Array(p >> 1);
  eq(ws.receive(id, b), b.length, 'received');
  return p & 1 ? dec.decode(b) : [...b];
};
// a socket made and open
async function opened(s, options) {
  const id = openOn(s, URL1, options);
  await settle();
  const fake = s.last();
  fake.protocol = 'chat';
  fake.extensions = 'permessage-deflate';
  fake.fire('open');
  s.signals.length = 0;
  return { id, fake };
}
const state = (ws, id) => ws.state(id) & 7;

// ---- the checks

await check('1 availability: 0 without WebSocket, 1, 2 with the network off', async () => {
  eq(make({ withWebSocket: false }).ws.availability(), 0, 'none');
  eq(make().ws.availability(), 1, 'ready');
  let on = true;
  const s = make({ network: () => on });
  on = false;
  eq(s.ws.availability(), 2, 'off');
  eq(openOn(s), 0, 'no open while off');
  eq(make({ withWebSocket: false }).ws.open(1, 2, 3, rec(URL1), 10, 10, 0), 0, 'no open without WebSocket');
});

await check('2 open: an id, no WebSocket and no signal inside it; after a turn the URL, the subprotocols, arraybuffer', async () => {
  const s = make();
  const id = openOn(s, URL1, { protocols: ['v2.chat', 'v1'] });
  assert(Number.isInteger(id) && id > 0, 'id ' + id);
  eq([s.WS.instances.length, s.signals], [0, []], 'nothing inside open');
  eq([s.ws.state(id), s.ws.infoSize(id), s.ws.peek(id)], [1, -1, -1], 'state 1, no info, nothing queued');
  await settle();
  const fake = s.last();
  eq([fake.url, fake.protocols, fake.binaryType], [URL1, ['v2.chat', 'v1'], 'arraybuffer'], 'the constructor');
  fake.fire('open');
  eq(fake.binaryTypeAtFirstEvent, 'arraybuffer', 'binaryType before any event');
  const none = openOn(s, 'wss://example.test/');
  await settle();
  eq(s.last().protocols, [], 'no subprotocol: an empty list');
  assert(none !== id, 'another id');
});

await check('3 32 open, the 33rd answers 0; ids start at random, wrap at 2^31-1, all different', async () => {
  const s = make({ Math: { floor: Math.floor, random: () => 0.9999999999 } });
  const ids = [];
  for (let i = 0; i < 32; i++) ids.push(openOn(s));
  eq(ids.slice(0, 3), [0x7fffffff, 1, 2], 'wraps from 0x7fffffff to 1');
  eq(new Set(ids).size, 32, 'all different');
  eq(openOn(s), 0, 'the 33rd');
  eq(s.ws.release(ids[5]), 1, 'release one');
  assert(openOn(s) > 0, 'room again');
  const a = make().ws.open(1, 2, 3, rec(URL1), 1, 1, 0), b = make().ws.open(1, 2, 3, rec(URL1), 1, 1, 0);
  assert(a !== b || a !== make().ws.open(1, 2, 3, rec(URL1), 1, 1, 0), 'random starts');
});

await check('4 request: http:, a fragment, a user, a CR, protocols alike ignoring case, the constructor\'s exceptions; off; the site\'s own host', async () => {
  let on = true;
  const s = make({ network: () => on });
  const kindOf = async (url, protocols = [], { throws = null, network = true } = {}) => {
    s.WS.throwNext = throws;
    const id = openOn(s, url, { protocols });
    on = network;
    await settle();
    on = true;
    eq(state(s.ws, id), 5, url + ': state');
    return info(s.ws, id).slice(0, 2);
  };
  const n = s.WS.instances.length;
  for (const [url, protocols, why] of [
    ['http://example.test/', [], /not a ws: or wss: URL: http:/],
    ['wss://example.test/#f', [], /a fragment/],
    ['wss://example.test/#', [], /a fragment/],
    ['ws://u:p@example.test/', [], /user name or a password/],
    ['ws://u@example.test/', [], /user name or a password/],
    ['ws://example.test/a\rb', [], /a CR, a LF or a NUL/],
    ['ws://example.test/a\0b', [], /a CR, a LF or a NUL/],
    ['ws://example.test/', ['a', 'A'], /offered twice/],
    ['ws://example.test/', ['a b'], /not a subprotocol/],
    ['ws://example.test/', [''], /not a subprotocol/],
    ['not a URL', [], /Invalid URL|invalid/i],
  ]) {
    const [kind, message] = await kindOf(url, protocols);
    eq(kind, 'request', url);
    assert(why.test(message), `${JSON.stringify(url)}: ${message}`);
  }
  eq(s.WS.instances.length, n, 'no WebSocket made for any of them');
  const syntax = Object.assign(new Error('bad'), { name: 'SyntaxError' });
  eq(await kindOf(URL1, [], { throws: syntax }), ['request', 'SyntaxError: bad'], 'SyntaxError');
  const security = () => Object.assign(new Error('mixed content'), { name: 'SecurityError' });
  const [kind, message] = await kindOf('ws://example.test/', [], { throws: security() });
  eq(kind, 'request', 'SecurityError while on');
  assert(/an https: page reaches only wss:/.test(message) && /SecurityError/.test(message), message);
  // the guard of vm-worker.js throws once the network is off, after the check of the service
  on = true;
  let calls = 0;
  const t = make({ network: () => calls++ < 2 });
  t.WS.throwNext = security();
  const id = openOn(t);
  await settle();
  eq(info(t.ws, id).slice(0, 2), ['off', 'network access is off'], 'SecurityError while off');
  eq((await kindOf(URL1, [], { network: false }))[0], 'off', 'off before the constructor');
  // the site's own host, ignoring the port, only in a browser
  const b = make({ isBrowser: true, siteHostname: 'pharo.example' });
  const own = openOn(b, 'wss://pharo.example:8443/ws');
  const other = openOn(b, 'wss://other.example/ws');
  await settle();
  eq(info(b.ws, own).slice(0, 2), ['request', 'the host of this site is refused: its cookies would be sent'], 'own host');
  eq(state(b.ws, other), 1, 'another host');
  const n2 = make({ isBrowser: false, siteHostname: 'pharo.example' });
  const nodeId = openOn(n2, 'wss://pharo.example/ws');
  await settle();
  eq(state(n2.ws, nodeId), 1, 'not in node');
  const bad = make();
  const notUtf8 = bad.ws.open(1, 2, 3, new Uint8Array([0x77, 0xc3]), 10, 10, 0);
  await settle();
  eq(info(bad.ws, notUtf8).slice(0, 2), ['request', 'the open record is not UTF-8'], 'not UTF-8');
});

await check('5 the open event: state 2, state, read and write signalled once each, the info record', async () => {
  const s = make();
  const id = openOn(s, URL1, { protocols: ['chat'] });
  await settle();
  const fake = s.last();
  fake.protocol = 'chat';
  fake.extensions = 'permessage-deflate; client_max_window_bits=15';
  fake.fire('open');
  eq([state(s.ws, id), s.signals], [2, [11, 12, 13]], 'state 2 and the signals');
  eq(info(s.ws, id), ['chat', 'permessage-deflate; client_max_window_bits=15', URL1], 'info');
  fake.fire('open');
  eq(s.signals, [11, 12, 13], 'a second open event changes nothing');
});

await check('6 messages: binary queued, peek 2 * size, receive copies and drops; text café 2 * 5 + 1; empty ones', async () => {
  const s = make();
  const { id, fake } = await opened(s);
  fake.fire('message', { data: new Uint8Array([1, 2, 3]).buffer });
  fake.fire('message', { data: 'café' });
  fake.fire('message', { data: '' });
  fake.fire('message', { data: new ArrayBuffer(0) });
  eq(s.ws.state(id), 2 + 8 * 4, 'four queued');
  eq(s.ws.peek(id), 6, 'binary of 3');
  eq(s.ws.receive(id, new Uint8Array(2)), -1, 'a dest of another size');
  eq(take(s.ws, id), [1, 2, 3], 'binary');
  eq(s.ws.peek(id), 11, 'text of 5 bytes');
  eq(take(s.ws, id), 'café', 'text');
  eq([s.ws.peek(id), take(s.ws, id)], [1, ''], 'empty text');
  eq([s.ws.peek(id), take(s.ws, id)], [0, []], 'empty binary');
  eq([s.ws.peek(id), s.ws.receive(id, new Uint8Array(0)), s.ws.state(id)], [-1, -1, 2], 'none left');
  eq([s.ws.peek(999), s.ws.receive(999, new Uint8Array(0))], [-1, -1], 'unknown id');
});

await check('7 read is signalled once when the queue goes from empty to one, not for the second; never state nor write', async () => {
  const s = make();
  const { id, fake } = await opened(s);
  fake.fire('message', { data: 'a' });
  fake.fire('message', { data: 'b' });
  eq(s.signals, [12], 'one read');
  take(s.ws, id);
  take(s.ws, id);
  fake.fire('message', { data: 'c' });
  eq(s.signals, [12, 12], 'again after it was emptied');
});

await check('8 tooLarge without encoding, overflow, a large one into an empty queue, the queues of all sockets', async () => {
  const s = make();
  const { id, fake } = await opened(s, { maxMessage: 10, maxQueued: 15 });
  fake.fire('message', { data: 'ok' });
  const encode = s.ctx.TextEncoder.prototype.encode;
  let encoded = 0;
  s.ctx.TextEncoder.prototype.encode = function (x) { if (x === 'x'.repeat(11)) encoded++; return encode.call(this, x); };
  try { fake.fire('message', { data: 'x'.repeat(11) }); } finally { s.ctx.TextEncoder.prototype.encode = encode; }
  eq([state(s.ws, id), encoded], [5, 0], 'tooLarge, never encoded');
  eq(info(s.ws, id), ['tooLarge', 'a message of more than 10 bytes', 'chat', 'permessage-deflate', URL1], 'the error');
  eq(fake.closes, [[4009, 'message too large']], 'closed with 4009');
  eq(take(s.ws, id), 'ok', 'the earlier message stays readable');
  const t = make();
  const b = await opened(t, { maxMessage: 10, maxQueued: 15 });
  b.fake.fire('message', { data: 'é'.repeat(6) });        // 6 units, 12 bytes
  eq(info(t.ws, b.id)[0], 'tooLarge', 'checked again after encoding');
  const u = make();
  const c = await opened(u, { maxMessage: 10, maxQueued: 15 });
  c.fake.fire('message', { data: new ArrayBuffer(10) });
  c.fake.fire('message', { data: new ArrayBuffer(5) });
  eq(state(u.ws, c.id), 2, '15 fit');
  c.fake.fire('message', { data: new ArrayBuffer(1) });
  eq([info(u.ws, c.id).slice(0, 2), c.fake.closes], [['overflow', 'more than 15 bytes or 65536 messages unread'],
    [[4008, 'too many unread messages']]], 'overflow');
  eq([u.ws.state(c.id) >> 3, take(u.ws, c.id).length, take(u.ws, c.id).length], [2, 10, 5], 'the queue stays readable');
  const v = make();
  const d = await opened(v, { maxMessage: 10, maxQueued: 4 });
  d.fake.fire('message', { data: new ArrayBuffer(9) });
  eq(state(v.ws, d.id), 2, 'a message of at most maxMessage always enters an empty queue');
  const w = make({ maxQueuedTotal: 25 });
  const socks = [];
  for (let i = 0; i < 3; i++) socks.push(await opened(w, { maxMessage: 10, maxQueued: 100 }));
  socks[0].fake.fire('message', { data: new ArrayBuffer(10) });
  socks[1].fake.fire('message', { data: new ArrayBuffer(10) });
  socks[2].fake.fire('message', { data: new ArrayBuffer(10) });
  eq(socks.map(x => state(w.ws, x.id)), [2, 2, 5], 'the third crosses the total');
  eq(info(w.ws, socks[2].id).slice(0, 2), ['overflow', 'the queues of all sockets are full'], 'its message');
  take(w.ws, socks[0].id);
  const e = await opened(w, { maxMessage: 10, maxQueued: 100 });
  e.fake.fire('message', { data: new ArrayBuffer(10) });
  eq(state(w.ws, e.id), 2, 'room again after a receive');
  w.ws.release(socks[1].id);
  const f = await opened(w, { maxMessage: 10, maxQueued: 100 });
  f.fake.fire('message', { data: new ArrayBuffer(5) });
  eq(state(w.ws, f.id), 2, 'room again after a release');
});

await check('9 send: -2 before the open and after a close frame of the server, a copy, -4, the BOM kept', async () => {
  const s = make();
  const id = openOn(s);
  eq(s.ws.send(id, new Uint8Array([1]), false), -2, 'before the WebSocket');
  await settle();
  const fake = s.last();
  eq(s.ws.send(id, new Uint8Array([1]), false), -2, 'while CONNECTING');
  fake.fire('open');
  const src = new Uint8Array([1, 2, 3]);
  eq(s.ws.send(id, src, false), 3, 'bufferedAmount after');
  src[0] = 9;
  eq([...fake.sent[0]], [1, 2, 3], 'a copy');
  eq(s.ws.send(id, new Uint8Array([0xc3, 0x28]), true), -4, 'not UTF-8');
  eq(s.ws.send(id, new Uint8Array(64 * MiB + 1), false), -4, 'over 64 MiB');
  eq(s.ws.send(id, new Uint8Array([0xef, 0xbb, 0xbf, 0x61]), true), 7, 'a BOM');
  eq([fake.sent[1].length, fake.sent[1].charCodeAt(0)], [2, 0xfeff], 'kept: 2 units');
  eq(s.ws.send(id, enc.encode('ciao'), true), 11, 'text');
  eq(fake.sent[2], 'ciao', 'as a string');
  eq(s.ws.send(999, src, false), -1, 'unknown id');
  fake.readyState = 2;
  eq(s.ws.send(id, src, false), -2, 'readyState 2 with no event');
  eq(state(s.ws, id), 3, 'which makes the state 3');
});

await check('10 back-pressure and the ticker: -3, one message on an empty buffer, write at LOW_WATER and 0, a deaf peer', async () => {
  const s = make();
  const { id, fake } = await opened(s);
  eq(s.ws.send(id, new Uint8Array(20 * MiB), false), 20 * MiB, 'one message of any size on an empty buffer');
  eq(s.ws.send(id, new Uint8Array(1), false), -3, 'full');
  eq(fake.sent.length, 1, 'nothing sent');
  fake.bufferedAmount = 500 * 1024;
  await s.timers.advance(16);
  eq(s.signals, [13], 'write at LOW_WATER');
  eq(s.ws.send(id, new Uint8Array(1024), false), 500 * 1024 + 1024, 'room again');
  fake.bufferedAmount = 0;
  await s.timers.advance(16);
  eq(s.signals, [13, 13], 'write at 0');
  eq(s.timers.delays(), [500], 'then the ticker only watches, at 500 ms');
  // 500 KB drained to 0: one signal
  s.signals.length = 0;
  s.ws.send(id, new Uint8Array(500 * 1024), false);
  fake.bufferedAmount = 100;
  await s.timers.advance(16);
  eq(s.signals, [], 'no signal on the way');
  fake.bufferedAmount = 0;
  await s.timers.advance(16);
  eq(s.signals, [13], 'one at 0');
  // 4 MiB: one at LOW_WATER and one at 0
  s.signals.length = 0;
  s.ws.send(id, new Uint8Array(4 * MiB), false);
  fake.bufferedAmount = 2 * MiB;
  await s.timers.advance(16);
  fake.bufferedAmount = MiB / 2;
  await s.timers.advance(16);
  fake.bufferedAmount = 0;
  await s.timers.advance(16);
  eq(s.signals, [13, 13], 'one at LOW_WATER, one at 0');
  // a deaf peer: no progress for 1 s, then 250 ms
  s.signals.length = 0;
  fake.grows = false;
  fake.bufferedAmount = 3 * MiB;
  s.ws.send(id, new Uint8Array(1), false);
  await s.timers.advance(1100);
  eq(s.timers.delays()[0] <= 250 && s.timers.delays()[0] > 16, true, 'ticks every 250 ms: ' + s.timers.delays());
  // stuck in CLOSING: the ticker signals write and stops draining
  fake.readyState = 2;
  await s.timers.advance(250);
  eq(s.signals, [13], 'write when readyState is 2');
  eq(state(s.ws, id), 3, 'state 3');
  s.ws.release(id);
  eq(s.timers.count, 0, 'no timer once released');
});

await check('11 close(1000, bye): 1, state 3, no signal; the close event: 4 and all three signalled; again 0; -1; -2', async () => {
  const s = make();
  const { id, fake } = await opened(s);
  fake.fire('message', { data: 'kept' });
  s.signals.length = 0;
  eq(s.ws.close(id, 1000, enc.encode('bye')), 1, 'started');
  eq([state(s.ws, id), s.signals, fake.closes], [3, [], [[1000, 'bye']]], 'state 3, no signal');
  eq(s.ws.close(id, 1000, new Uint8Array(0)), 0, 'a second close');
  fake.fire('close', { code: 1000, reason: 'bye', wasClean: true });
  eq([state(s.ws, id), s.signals], [4, [11, 12, 13]], 'state 4, all three');
  eq(info(s.ws, id), ['chat', 'permessage-deflate', URL1, '1000', 'bye', '1'], 'info');
  eq(take(s.ws, id), 'kept', 'the queue stays readable');
  eq(s.ws.close(id, 1000, new Uint8Array(0)), 0, 'closed: 0');
  eq(s.ws.close(999, 1000, new Uint8Array(0)), -1, 'unknown id');
  const t = make();
  const b = await opened(t);
  eq(t.ws.close(b.id, 1000, new Uint8Array([0xc3])), -2, 'a reason that is not UTF-8');
  eq([state(t.ws, b.id), b.fake.closes], [2, []], 'nothing done');
  b.fake.close = () => { throw Object.assign(new Error('code'), { name: 'InvalidAccessError' }); };
  eq(t.ws.close(b.id, 1001, new Uint8Array(0)), -2, 'a code that the WebSocket refuses');
  eq(state(t.ws, b.id), 2, 'still open');
  const r = make();
  const c = await opened(r);
  r.ws.close(c.id, 4000, enc.encode('ciao è'));
  c.fake.fire('close', { code: 4000, reason: 'a\nb\rc\0d', wasClean: false });
  eq(info(r.ws, c.id).slice(3), ['4000', 'a b c d', '0'], 'CR, LF and NUL of a reason become spaces');
  eq(c.fake.closes, [[4000, 'ciao è']], 'the reason decoded');
});

await check('12 a close of the server: readyState 2 makes state 3 and arms the close timer; 4 by the event or after CLOSE_TIMEOUT', async () => {
  const s = make();
  const { id, fake } = await opened(s);
  fake.readyState = 2;
  eq(state(s.ws, id), 3, 'state 3');
  eq(s.signals, [], 'no signal');
  fake.fire('close', { code: 4321, reason: 'bye', wasClean: true });
  eq([state(s.ws, id), info(s.ws, id).slice(3)], [4, ['4321', 'bye', '1']], 'the event');
  const t = make();
  const b = await opened(t);
  b.fake.readyState = 2;
  t.ws.state(b.id);
  await t.timers.advance(4999);
  eq(state(t.ws, b.id), 3, 'not before 5 s');
  await t.timers.advance(1);
  eq([state(t.ws, b.id), info(t.ws, b.id).slice(3), t.signals], [4, ['1006', 'closing handshake timed out', '0'], [11, 12, 13]],
    'after CLOSE_TIMEOUT');
  // nobody asks: the ticker sees it
  const u = make();
  const c = await opened(u);
  c.fake.readyState = 2;
  await u.timers.advance(500);
  await u.timers.advance(5000);
  eq(state(u.ws, c.id), 4, 'seen by the ticker, ended by the close timer');
  // the image closes, the server never answers
  const v = make();
  const d = await opened(v);
  v.ws.close(d.id, 1000, new Uint8Array(0));
  await v.timers.advance(5000);
  eq([state(v.ws, d.id), info(v.ws, d.id).slice(3, 5)], [4, ['1006', 'closing handshake timed out']], 'close() with no answer');
  eq(v.timers.count, 0, 'no timer left');
});

await check('13 the transition table: error and close before the open, after it, and a close during CONNECTING', async () => {
  const s = make();
  let id = openOn(s);
  await settle();
  s.last().fire('error', {});
  s.last().fire('close', { code: 1006, reason: '', wasClean: false });
  eq(info(s.ws, id).slice(0, 2), ['network', 'the handshake failed (no answer, a refusal, or a status other than 101)'], 'error, close');
  eq(s.signals, [11, 12, 13], 'signalled once');
  id = openOn(s);
  await settle();
  s.last().fire('error', { message: 'ECONNREFUSED', error: { cause: { code: 'ECONNREFUSED' } } });
  eq(info(s.ws, id)[1], 'the handshake failed (no answer, a refusal, or a status other than 101): ECONNREFUSED: ECONNREFUSED', 'what an event says');
  id = openOn(s);
  await settle();
  s.last().fire('close', { code: 1006, reason: '', wasClean: false });
  eq(info(s.ws, id)[0], 'network', 'close alone before the open');
  const t = make();
  const b = await opened(t);
  b.fake.fire('error', {});
  eq([state(t.ws, b.id), t.signals], [2, []], 'error after the open: ignored');
  b.fake.fire('close', { code: 1006, reason: '', wasClean: false });
  eq([state(t.ws, b.id), info(t.ws, b.id).slice(3)], [4, ['1006', '', '0']], 'then close: 4, not clean');
  const u = make();
  id = openOn(u);
  await settle();
  eq(u.ws.close(id, 4000, new Uint8Array(0)), 1, 'close during CONNECTING');
  eq([state(u.ws, id), u.last().closes], [3, [[4000, '']]], 'state 3');
  u.last().fire('error', {});
  u.last().fire('close', { code: 1006, reason: '', wasClean: false });
  eq([state(u.ws, id), info(u.ws, id).slice(3)], [4, ['1006', 'closed before it was open', '0']], '4, not 5');
  u.last().fire('open');
  u.last().fire('message', { data: 'late' });
  eq(u.ws.state(id), 4, 'nothing after 4');
});

await check('14 a close before the WebSocket was made: 1, none made, state 4 from a microtask, signalled', async () => {
  const s = make();
  const id = openOn(s);
  eq(s.ws.close(id, 1000, new Uint8Array(0)), 1, 'started');
  eq([state(s.ws, id), s.signals], [3, []], 'state 3 inside');
  await settle();
  eq([s.WS.instances.length, state(s.ws, id), s.signals], [0, 4, [11, 12, 13]], 'none made, 4');
  eq(info(s.ws, id).slice(3), ['1006', 'closed before it was open', '0'], 'info');
});

await check('15 the handshake timeout; none after the open; late events after a timeout, a networkOff, a release', async () => {
  const s = make();
  const id = openOn(s, URL1, { timeout: 500 });
  await settle();
  await s.timers.advance(500);
  eq([info(s.ws, id).slice(0, 2), s.last().closes], [['timeout', 'not open after 500 ms'], [[1000, 'timeout']]], 'timeout');
  s.signals.length = 0;
  for (const [type, init] of [['open', {}], ['error', {}], ['message', { data: 'x' }], ['close', { code: 1006 }]]) s.last().fire(type, init);
  eq([s.ws.state(id), s.signals], [5, []], 'nothing after it');
  const t = make();
  const b = await opened(t, { timeout: 500 });
  await t.timers.advance(1000);
  eq(state(t.ws, b.id), 2, 'no timeout after the open');
  let on = true;
  const u = make({ network: () => on });
  const c = await opened(u);
  on = false;
  u.ws.networkOff();
  u.signals.length = 0;
  for (const [type, init] of [['error', {}], ['message', { data: 'x' }], ['close', { code: 1000 }]]) c.fake.fire(type, init);
  eq([u.ws.state(c.id), u.signals], [5, []], 'nothing after networkOff');
  const v = make();
  const d = await opened(v);
  v.ws.release(d.id);
  for (const [type, init] of [['error', {}], ['message', { data: 'x' }], ['close', { code: 1000 }]]) d.fake.fire(type, init);
  eq([v.ws.state(d.id), v.signals], [-1, []], 'nothing after release');
});

await check('16 release: -1 after it, closed with 1000 released, no signal any more, lingering until its close event', async () => {
  const s = make();
  const { id, fake } = await opened(s);
  fake.fire('message', { data: 'lost' });
  eq([s.ws.release(id), s.ws.release(id)], [1, 0], 'known, then not');
  eq([s.ws.state(id), s.ws.peek(id), s.ws.infoSize(id), s.ws.buffered(id), s.ws.drop(id)], [-1, -1, -1, -1, -1], 'unknown');
  eq(fake.closes, [[1000, 'released']], 'closed');
  eq(s.ws.lingering(), 1, 'lingering');
  s.signals.length = 0;
  fake.fire('message', { data: 'late' });
  fake.fire('close', { code: 1000, reason: 'released', wasClean: true });
  eq([s.ws.lingering(), s.signals], [0, []], 'its close event ends it, signalling nothing');
  const t = make();
  const b = await opened(t);
  t.ws.close(b.id, 1000, new Uint8Array(0));
  t.ws.release(b.id);
  eq(b.fake.closes, [[1000, '']], 'closing already: not closed again');
  const u = make();
  const c = await opened(u);
  c.fake.fire('close', { code: 1000, reason: '', wasClean: true });
  u.ws.release(c.id);
  eq(u.ws.lingering(), 0, 'a CLOSED one does not linger');
});

await check('17 lingering: 32 releases of sockets that never close, then open answers 0; one close event, then it works', async () => {
  const s = make();
  const fakes = [];
  for (let i = 0; i < 32; i++) {
    const { id, fake } = await opened(s);
    fakes.push(fake);
    s.ws.release(id);
  }
  eq([s.ws.lingering(), openOn(s)], [32, 0], 'all slots taken');
  fakes[7].fire('close', { code: 1006, reason: '', wasClean: false });
  eq(s.ws.lingering(), 31, 'one closed');
  assert(openOn(s) > 0, 'room again');
});

await check('18 networkOff: 1, 2 and 3 fail with off, their queues kept, closed with 1000; 4 stays; through pharoHost.setNetwork', async () => {
  const ctx = context();
  const signals = [];
  ctx.Module = { _vm_signal_semaphore: i => { signals.push(i); return 1; } };
  vm.runInContext(hostEventsSrc, ctx, { filename: 'host-events.js' });
  const WS = fakeWebSocketClass();
  ctx.WebSocket = WS;
  vm.runInContext(webSocketSrc, ctx, { filename: 'web-socket.js' });
  const ws = ctx.Module.webSocket;
  eq(ws.availability(), 1, 'ready');
  const ids = [1, 2, 3, 4].map(() => ws.open(11, 12, 13, rec(URL1), 100, 100, 0));
  await settle();
  WS.instances[1].fire('open');
  WS.instances[1].fire('message', { data: 'kept' });
  WS.instances[2].fire('open');
  ws.close(ids[2], 1000, new Uint8Array(0));
  WS.instances[3].fire('open');
  WS.instances[3].fire('close', { code: 1000, reason: '', wasClean: true });
  signals.length = 0;
  ctx.Module.pharoHost.setNetwork(false);
  eq(ids.map(id => ws.state(id) & 7), [5, 5, 5, 4], 'the states');
  eq(ids.slice(0, 3).map(id => info(ws, id).slice(0, 2).join(': ')), Array(3).fill('off: network access is off'), 'the errors');
  eq(WS.instances.map(w => w.closes), [[[1000, 'network access is off']], [[1000, 'network access is off']], [[1000, '']], []], 'closed with 1000');
  eq(take(ws, ids[1]), 'kept', 'the queue kept');
  eq(signals.length, 9, 'all three of each');
  eq([ws.availability(), ws.open(1, 2, 3, rec(URL1), 1, 1, 0)], [2, 0], 'off');
  ctx.Module.pharoHost.setNetwork(true);
  assert(ws.open(1, 2, 3, rec(URL1), 1, 1, 0) > 0, 'on again');
});

await check('19 shutdown: all forgotten and closed, nothing signals, no timer pending, open 0 after it, lingering', async () => {
  const s = make();
  const a = await opened(s, { timeout: 100 });
  a.fake.readyState = 2;
  s.ws.state(a.id);
  const b = await opened(s);
  s.ws.send(b.id, new Uint8Array(100), false);
  const c = openOn(s, URL1, { timeout: 1000 });
  await settle();
  const cFake = s.last();
  const d = await opened(s);
  d.fake.fire('close', { code: 1000, reason: '', wasClean: true });
  assert(s.timers.count > 0, 'timers before');
  s.signals.length = 0;
  s.ws.shutdown();
  eq([s.timers.count, s.signals], [0, []], 'no timer, no signal');
  eq([a.id, b.id, c, d.id].map(id => s.ws.state(id)), [-1, -1, -1, -1], 'forgotten');
  eq([a.fake.closes, b.fake.closes, cFake.closes, d.fake.closes], [[], [[1000, 'the VM ended']], [[1000, 'the VM ended']], []],
    'closed unless closing');
  eq(s.ws.lingering(), 3, 'those not CLOSED linger');
  eq(openOn(s), 0, 'no open after it');
  b.fake.fire('close', { code: 1000 });
  eq([s.ws.lingering(), s.signals], [2, []], 'a late close event');
});

await check('20 drop: the size, then tooLarge closed with 4009; the next message is still there', async () => {
  const s = make();
  const { id, fake } = await opened(s);
  fake.fire('message', { data: new ArrayBuffer(7) });
  fake.fire('message', { data: 'next' });
  s.signals.length = 0;
  eq(s.ws.drop(id), 7, 'its size');
  eq([state(s.ws, id), info(s.ws, id).slice(0, 2), fake.closes],
    [5, ['tooLarge', 'a message of 7 bytes is larger than the image can hold'], [[4009, 'message too large']]], 'failed');
  eq(s.signals, [11, 12, 13], 'signalled');
  eq([take(s.ws, id), s.ws.drop(id)], ['next', -1], 'the next one, then none');
  const t = make();
  const b = await opened(t);
  b.fake.fire('message', { data: 'a' });
  b.fake.fire('message', { data: 'b' });
  b.fake.fire('close', { code: 1000, reason: '', wasClean: true });
  eq([t.ws.drop(b.id), state(t.ws, b.id)], [1, 4], 'a closed socket stays closed');
});

await check('21 no method throws for an unknown id; the file declares no binding named WebSocket; the timers are unref\'d', async () => {
  const s = make();
  const answers = [s.ws.state(5), s.ws.infoSize(5), s.ws.info(5, new Uint8Array(1)), s.ws.peek(5), s.ws.receive(5, new Uint8Array(1)),
    s.ws.drop(5), s.ws.send(5, new Uint8Array(1), true), s.ws.buffered(5), s.ws.close(5, 1000, new Uint8Array(0)), s.ws.release(5)];
  eq(answers, [-1, -1, -1, -1, -1, -1, -1, -1, -1, 0], 'answers');
  s.ws.networkOff();
  s.ws.shutdown();
  s.ws.shutdown();
  const code = webSocketSrc.replace(/\/\/.*$/gm, '');
  assert(!/\b(var|let|const|function|class)\s+WebSocket\b/.test(code), 'a binding named WebSocket');
  assert(!/[{,]\s*WebSocket\s*[,}]/.test(code.replace(/WebSocket:\s/g, '')), 'a shorthand WebSocket binding');
  const t = make();
  await opened(t, { timeout: 100 });
  assert(t.timers.made > 0 && t.timers.unrefs === t.timers.made, `${t.timers.unrefs} of ${t.timers.made} unref'd`);
});

await check('22 node-pump.js: the exit guard only with lingering sockets, unref\'d, exiting with the status once the output is out', async () => {
  // a VM that ends at its first slice, with these WebSockets lingering
  const run = async (webSocket, { trap = false, stats = false } = {}) => {
    const timers = fakeTimers(), log = [];
    const Module = {
      pharoHost: { shutdown() { log.push('shutdown'); } },
      _vm_wakeup_ms: () => 1,
      _vm_stats() { log.push('stats'); },
      _vm_resume() {
        if (trap) throw new Error('trap');
        throw Object.assign(new Error('exit'), { name: 'ExitStatus' });
      },
    };
    if (webSocket !== undefined) Module.webSocket = { lingering: () => { log.push('lingering'); return webSocket; } };
    const writer = name => ({ write(text, done) { log.push(name + ' ' + JSON.stringify(text)); if (done) setImmediate(done); } });
    const proc = {
      env: stats ? { PHARO_WASM_STATS: '1' } : {}, exitCode: trap ? undefined : 3,
      stdout: writer('stdout'), stderr: writer('stderr'),
      exit(code) { log.push('exit ' + code); },
    };
    const ctx = vm.createContext({ Module, setImmediate, clearImmediate, clearTimeout: h => timers.clearTimeout(h),
      setTimeout: (f, ms) => (Number.isFinite(ms) && ms >= 1000 ? timers.setTimeout(f, ms) : setTimeout(f, ms)), process: proc });
    vm.runInContext(nodePumpSrc, ctx, { filename: 'node-pump.js' });
    Module.onVMStarted();
    await settle();
    const armed = timers.delays();
    await timers.advance(1500);
    await settle();
    return { armed, unrefs: timers.unrefs, log };
  };
  const none = await run(undefined);
  eq([none.armed, none.log], [[], ['shutdown']], 'no service: nothing armed');
  const zero = await run(0, { stats: true });
  eq([zero.armed, zero.log], [[], ['shutdown', 'lingering', 'stats', 'stderr "[pharo-wasm] exitGuard=0\\n"']], 'none lingering');
  const one = await run(1, { stats: true });
  eq([one.armed, one.unrefs], [[1500], 1], 'one lingering: 1500 ms, unref\'d');
  eq(one.log, ['shutdown', 'lingering', 'stats', 'stderr "[pharo-wasm] exitGuard=1\\n"', 'stdout ""', 'stderr ""', 'exit 3'],
    'the output first, then the exit with the status');
  const trapped = await run(2, { trap: true });
  eq(trapped.log.slice(-1), ['exit 1'], 'a trap: the status set after the shutdown');
});

await check('23 a WebSocket of node closed while connecting (its events inside close(), readyState left at 2): no signal inside a method, no lingering', async () => {
  const s = make();
  s.WS.nodeLike = true;
  // the handshake timer, then a release, as the package does after it settles
  const a = openOn(s, URL1, { timeout: 100 });
  await settle();
  await s.timers.advance(100);
  eq([state(s.ws, a), info(s.ws, a).slice(0, 2), s.signals], [5, ['timeout', 'not open after 100 ms'], [11, 12, 13]], 'timeout');
  eq(s.last().closes, [[1000, 'timeout']], 'closed with 1000 timeout');
  eq([s.ws.release(a), s.ws.lingering()], [1, 0], 'released, not lingering: its close event came');
  // a release while connecting
  const b = openOn(s);
  await settle();
  s.signals.length = 0;
  eq([s.ws.release(b), s.signals, s.ws.lingering()], [1, [], 0], 'release: nothing signalled, not lingering');
  await settle();
  eq(s.signals, [], 'nothing after it');
  // a close while connecting: 4, closed before it was open, from a microtask
  const c = openOn(s);
  await settle();
  s.signals.length = 0;
  eq([s.ws.close(c, 1000, new Uint8Array(0)), s.signals, state(s.ws, c)], [1, [], 3], 'close: 1, nothing signalled inside it, state 3');
  await settle();
  eq([state(s.ws, c), info(s.ws, c).slice(3), s.signals], [4, ['1006', 'closed before it was open', '0'], [11, 12, 13]], 'then 4, signalled once');
  eq(s.ws.release(c), 1, 'release');
  // 40 timeouts and releases: never refused
  const ids = [];
  for (let i = 0; i < 40; i++) {
    const id = openOn(s, URL1, { timeout: 50 });
    ids.push(id);
    await settle();
    await s.timers.advance(50);
    s.ws.release(id);
  }
  eq([ids.filter(id => id > 0).length, s.ws.lingering(), s.timers.count], [40, 0, 0], 'all 40 opened, none lingering, no timer');
});

await check('24 node\'s own WebSocket against a server that never answers the handshake: timeouts, releases and a close, no lingering, the TCP ends', async () => {
  const net = await import('node:net');
  const conns = new Set();
  const server = net.createServer(c => { conns.add(c); c.resume(); c.on('error', () => {}); c.on('close', () => conns.delete(c)); });
  await new Promise(r => server.listen(0, '127.0.0.1', r));
  try {
    const ctx = context();
    vm.runInContext(webSocketSrc, ctx, { filename: 'web-socket.js' });
    const signals = [];
    const ws = ctx.PharoWebSocket.create({ signal: i => { signals.push(i); return true; }, WebSocket: () => globalThis.WebSocket });
    const url = `ws://127.0.0.1:${server.address().port}/ws`;
    const until = async (what, f, ms = 5000) => {
      const end = Date.now() + ms;
      while (!f()) {
        if (Date.now() > end) throw new Error('timed out waiting for ' + what);
        await new Promise(r => setTimeout(r, 10));
      }
    };
    let opened = 0;
    for (let i = 0; i < 34; i++) {
      const id = ws.open(1, 2, 3, rec(url), 1000, 1000, 50);
      if (!id) break;
      opened++;
      await until('the timeout', () => (ws.state(id) & 7) >= 4);
      eq(info(ws, id)[0], 'timeout', 'kind');
      ws.release(id);
    }
    eq([opened, ws.lingering()], [34, 0], 'all 34 opened, none lingering');
    const c = ws.open(1, 2, 3, rec(url), 1000, 1000, 0);
    await until('the connection', () => conns.size > 0);
    signals.length = 0;
    eq([ws.close(c, 1000, new Uint8Array(0)), signals], [1, []], 'close while connecting: 1, nothing signalled inside it');
    await until('the end', () => (ws.state(c) & 7) >= 4, 1000);
    eq([ws.state(c) & 7, info(ws, c).slice(3)], [4, ['1006', 'closed before it was open', '0']], 'closed before it was open');
    ws.release(c);
    // node drops each connection at the abort; its pool may make a fresh
    // one then, which ends with its keep-alive timeout (4 s)
    await until('the TCP connections to end', () => conns.size === 0, 8000);
  } finally {
    for (const c of conns) c.destroy();
    await new Promise(r => server.close(r));
  }
});

console.log(`${passes} passed, ${failures} failed`);
if (failures > 0) process.exit(1);
