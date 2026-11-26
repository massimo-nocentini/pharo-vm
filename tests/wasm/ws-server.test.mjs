// ws-server.test.mjs - unit tests of lib/ws-server.mjs, the WebSocket endpoints of the test server
//
// usage: node tests/wasm/ws-server.test.mjs
//
// Starts lib/fetch-http.mjs (which attaches ws-server.mjs) on an ephemeral
// local port and checks it with node's own WebSocket and with a raw client
// over node:net, which sends frames that no WebSocket API can: unmasked,
// with RSV bits, with a huge length, control frames that are too long or
// fragmented.  Prints every check and exits 1 on failure.

import crypto from 'node:crypto';
import net from 'node:net';
import zlib from 'node:zlib';
import { startFetchHttp } from './lib/fetch-http.mjs';
import { acceptOf, frame, pushMessage, fragmentedMessage } from './lib/ws-server.mjs';

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

const sleep = ms => new Promise(r => setTimeout(r, ms));
async function until(what, pred, ms = 3000) {
  const t0 = performance.now();
  while (!pred()) {
    if (performance.now() - t0 > ms) throw new Error('timed out waiting for ' + what);
    await sleep(5);
  }
}

const srv = await startFetchHttp({ port: 0 });
const base = srv.url.replace(/^http:/, 'ws:');
const last = path => [...srv.ws].reverse().find(e => e.url === path || e.path === path);

// node's WebSocket: {ws, events: [...], messages: [...], opened, closed}, the
// close event as {code, reason, wasClean, at} (ms from the start)
function client(path, protocols = []) {
  const ws = new WebSocket(base + path, protocols);
  ws.binaryType = 'arraybuffer';
  const t0 = performance.now();
  const c = { ws, messages: [], read: 0, errors: 0, close: null };
  c.opened = new Promise((resolve, reject) => {
    ws.addEventListener('open', () => resolve());
    ws.addEventListener('close', () => reject(new Error('closed before the open')));
  });
  c.opened.catch(() => {});
  ws.addEventListener('message', e => c.messages.push(e.data));
  ws.addEventListener('error', () => { c.errors++; });
  c.closed = new Promise(resolve => ws.addEventListener('close', e => {
    c.close = { code: e.code, reason: e.reason, wasClean: e.wasClean, at: performance.now() - t0 };
    resolve(c.close);
  }));
  // the next message not taken yet; all of the first n
  c.next = async () => { await until('message ' + (c.read + 1), () => c.messages.length > c.read); return c.messages[c.read++]; };
  c.upTo = async n => { await until(n + ' messages', () => c.messages.length >= n); return c.messages.slice(0, n); };
  return c;
}
const bytesOf = data => typeof data === 'string' ? Buffer.from(data, 'utf8') : Buffer.from(new Uint8Array(data));

// A raw client: the handshake, then masked frames out and the server's
// frames in, {status, headers, frames: [{fin, rsv, opcode, payload}], ended}
function raw(path, { port = srv.port, key = crypto.randomBytes(16).toString('base64'), version = '13', headers = {}, handshake = true } = {}) {
  return new Promise((resolve, reject) => {
    const socket = net.connect(port, '127.0.0.1');
    const r = { socket, key, status: null, headers: {}, frames: [], ended: false, buf: Buffer.alloc(0) };
    socket.on('error', () => {});
    socket.on('close', () => { r.ended = true; });
    r.send = (opcode, payload = Buffer.alloc(0), options = {}) =>
      socket.write(frame(opcode, Buffer.from(payload), Object.assign({ mask: crypto.randomBytes(4) }, options)));
    r.write = bytes => socket.write(bytes);
    r.frame = async n => { await until('frame ' + n, () => r.frames.length >= n || r.ended); return r.frames[n - 1]; };
    const parse = () => {
      for (;;) {
        const b = r.buf;
        if (b.length < 2) return;
        let len = b[1] & 127, at = 2;
        if (len === 126) { if (b.length < 4) return; len = b.readUInt16BE(2); at = 4; }
        else if (len === 127) { if (b.length < 10) return; len = b.readUInt32BE(2) * 2 ** 32 + b.readUInt32BE(6); at = 10; }
        if (b.length < at + len) return;
        r.frames.push({ fin: (b[0] & 0x80) !== 0, rsv: (b[0] >> 4) & 7, opcode: b[0] & 15, payload: b.subarray(at, at + len) });
        r.buf = b.subarray(at + len);
      }
    };
    const lines = [`GET ${path} HTTP/1.1`, 'Host: 127.0.0.1', 'Upgrade: websocket', 'Connection: Upgrade',
      'Sec-WebSocket-Version: ' + version];
    if (key) lines.push('Sec-WebSocket-Key: ' + key);
    for (const [k, v] of Object.entries(headers)) lines.push(k + ': ' + v);
    socket.on('connect', () => socket.write(lines.join('\r\n') + '\r\n\r\n'));
    socket.on('data', d => {
      r.buf = Buffer.concat([r.buf, d]);
      if (r.status === null) {
        const end = r.buf.indexOf('\r\n\r\n');
        if (end < 0) return;
        const head = r.buf.subarray(0, end).toString('latin1').split('\r\n');
        r.status = Number(head[0].split(' ')[1]);
        for (const line of head.slice(1)) {
          const i = line.indexOf(':');
          r.headers[line.slice(0, i).trim().toLowerCase()] = line.slice(i + 1).trim();
        }
        r.buf = r.buf.subarray(end + 4);
        resolve(r);
      }
      parse();
    });
    if (!handshake) resolve(r);
    setTimeout(() => reject(new Error('no answer to the handshake')), 3000).unref();
  });
}
const closeCode = f => f && f.opcode === 8 && f.payload.length >= 2 ? f.payload.readUInt16BE(0) : null;

try {
  await check('1 the handshake: 101, the accept key, Upgrade', async () => {
    const r = await raw('/ws/echo');
    eq([r.status, r.headers.upgrade, r.headers.connection, r.headers['sec-websocket-accept']],
      [101, 'websocket', 'Upgrade', acceptOf(r.key)], 'head');
    eq(acceptOf('dGhlIHNhbXBsZSBub25jZQ=='), 's3pPLMBiTxaQ9kYGzzhZRbK+xOo=', 'the key of RFC 6455, 1.3');
    r.socket.destroy();
  });

  await check('2 400 without a key, 426 for another version, 404 for another path', async () => {
    eq((await raw('/ws/echo', { key: null })).status, 400, 'no key');
    const other = await raw('/ws/echo', { version: '8' });
    eq([other.status, other.headers['sec-websocket-version']], [426, '13'], 'version 8');
    eq((await raw('/ws/nothing')).status, 404, 'no such endpoint');
    eq((await raw('/hello')).status, 404, 'not under /ws/');
  });

  await check('3 the chosen subprotocol: the first offered that the endpoint takes; none fails the client', async () => {
    const c = client('/ws/echo', ['x', 'chat', 'echo']);
    await c.opened;
    eq(c.ws.protocol, 'chat', 'protocol');
    eq([last('/ws/echo').offered, last('/ws/echo').protocol], [['x', 'chat', 'echo'], 'chat'], 'log');
    c.ws.close();
    const plain = client('/ws/push?n=0&close=none');
    await plain.opened;
    eq(plain.ws.protocol, '', 'none offered, none chosen');
    plain.ws.close();
    // the server chose none of those offered: a client fails the connection
    // (WHATWG WebSockets, as node and the browsers do)
    const none = client('/ws/push?n=0&close=none', ['chat']);
    eq([await none.opened.then(() => 'open', () => 'failed'), (await none.closed).code, none.errors], ['failed', 1006, 1], 'push takes none');
    eq(last('/ws/push?n=0&close=none').offered, ['chat'], 'log');
    await Promise.all([c.closed, plain.closed]);
  });

  await check('4 echo of text and binary at 125, 126, 65535, 65536 bytes and 1 MiB', async () => {
    const c = client('/ws/echo');
    await c.opened;
    const sizes = [0, 125, 126, 65535, 65536, 1 << 20];
    const sent = [];
    for (const n of sizes) {
      const text = 'é'.repeat(n >> 1) + (n & 1 ? 'x' : '');
      const bin = new Uint8Array(n).map((_, j) => (j * 7) & 255);
      c.ws.send(text);
      c.ws.send(bin);
      sent.push(text, bin);
    }
    await c.upTo(sent.length);
    for (let i = 0; i < sent.length; i++) {
      const want = sent[i], got = c.messages[i];
      assert(typeof got === typeof want || (typeof want !== 'string' && got instanceof ArrayBuffer), 'type of ' + i);
      assert(bytesOf(got).equals(bytesOf(typeof want === 'string' ? want : want.buffer)), 'bytes of ' + i);
    }
    eq(bytesOf(c.messages[2]).length, 125, 'a text of 125 bytes');
    c.ws.close();
    await c.closed;
  });

  await check('5 an unmasked frame of the client: 1002', async () => {
    const r = await raw('/ws/echo');
    r.write(frame(1, Buffer.from('hello')));
    eq(closeCode(await r.frame(1)), 1002, 'close code');
    await until('the end of TCP', () => r.ended);
  });

  await check('6 text that is not UTF-8: 1007, also across two fragments', async () => {
    const r = await raw('/ws/echo');
    r.send(1, Buffer.from([0x61, 0xc3, 0x28]));
    eq(closeCode(await r.frame(1)), 1007, 'one frame');
    const s = await raw('/ws/echo');
    s.send(1, Buffer.from([0x61, 0xc3]), { fin: false });
    s.send(0, Buffer.from([0x28]));
    eq(closeCode(await s.frame(1)), 1007, 'two fragments');
  });

  await check('7 fragments joined: an é cut between two of them', async () => {
    const r = await raw('/ws/echo');
    const bytes = Buffer.from('caffè latte', 'utf8');
    r.send(1, bytes.subarray(0, 4), { fin: false });
    r.send(0, bytes.subarray(4, 5), { fin: false });
    r.send(0, bytes.subarray(5));
    const f = await r.frame(1);
    eq([f.opcode, f.fin, f.payload.toString('utf8')], [1, true, 'caffè latte'], 'the message');
    r.socket.destroy();
  });

  await check('8 a ping between fragments: the pong comes, then the message; node joins those of /ws/fragmented', async () => {
    const r = await raw('/ws/echo');
    r.send(2, Buffer.from([1, 2]), { fin: false });
    r.send(9, Buffer.from('between'));
    r.send(0, Buffer.from([3]));
    const pong = await r.frame(1), echo = await r.frame(2);
    eq([pong.opcode, pong.payload.toString()], [10, 'between'], 'pong');
    eq([echo.opcode, [...echo.payload]], [2, [1, 2, 3]], 'echo');
    r.socket.destroy();
    const c = client('/ws/fragmented?parts=4');
    eq(bytesOf(await c.next()).toString('utf8'), fragmentedMessage(4).toString('utf8'), 'text');
    const b = client('/ws/fragmented?parts=3&type=binary');
    assert(bytesOf(await b.next()).equals(fragmentedMessage(3, 'binary')), 'binary');
    const raws = await raw('/ws/fragmented?parts=3');
    await raws.frame(4);
    eq(raws.frames.map(f => [f.opcode, f.fin]), [[1, false], [9, true], [0, false], [0, true]], 'the frames, a ping in between');
    raws.socket.destroy();
    c.ws.close();
    b.ws.close();
    await Promise.all([c.closed, b.closed]);
  });

  await check('9 /ws/ping: the pong of node carries the payload', async () => {
    const c = client('/ws/ping?payload=ciao');
    await c.opened;
    await until('the pong', () => last('/ws/ping?payload=ciao').pongs.length > 0);
    eq(last('/ws/ping?payload=ciao').pongs, ['ciao'], 'pongs');
    c.ws.close();
    await c.closed;
  });

  await check('10 /ws/close: the code and the reason of the server, clean', async () => {
    const c = client('/ws/close?code=4321&reason=bye');
    eq(await c.closed, { code: 4321, reason: 'bye', wasClean: true, at: c.close.at }, 'close');
    const e = last('/ws/close?code=4321&reason=bye');
    await until('the log', () => e.ended);
    eq([e.sentCode, e.code, e.clean], [4321, 4321, true], 'log');
    const later = client('/ws/close?after=200');
    eq((await later.closed).code, 1000, 'after 200 ms');
    assert(later.close.at >= 190, 'not before ' + later.close.at);
  });

  await check('11 the abrupt end: 1006, not clean', async () => {
    const c = client('/ws/close?abrupt=1&after=50');
    await c.opened;
    const e = await c.closed;
    eq([e.code, e.wasClean], [1006, false], 'close');
    eq(last('/ws/close?abrupt=1&after=50').clean, false, 'log');
  });

  await check('12 broadcast to two sockets, the sender too', async () => {
    const a = client('/ws/broadcast?room=r1'), b = client('/ws/broadcast?room=r1'), c = client('/ws/broadcast?room=r2');
    await Promise.all([a.opened, b.opened, c.opened]);
    a.ws.send('to all');
    eq([await a.next(), await b.next()], ['to all', 'to all'], 'both');
    await sleep(50);
    eq(c.messages, [], 'not another room');
    for (const x of [a, b, c]) x.ws.close();
    await Promise.all([a.closed, b.closed, c.closed]);
  });

  await check('13 /_stats lists the connections in ws, filtered by tag, and /_reset clears them', async () => {
    const c = client('/ws/echo?stats-tag');
    await c.opened;
    c.ws.close(4000, 'done');
    await c.closed;
    await until('the log', () => last('/ws/echo?stats-tag').ended);
    const stats = await (await fetch(srv.url + '/_stats?tag=stats-tag')).json();
    eq(stats.ws.map(e => [e.path, e.code, e.reason, e.clean]), [['/ws/echo', 4000, 'done', true]], 'ws');
    eq((await fetch(srv.url + '/_reset', { method: 'POST' })).status, 204, '/_reset');
    eq((await (await fetch(srv.url + '/_stats')).json()).ws, [], 'empty after /_reset');
  });

  await check('14 after a close, node gets its close event within 500 ms: the server ends TCP', async () => {
    for (const path of ['/ws/echo', '/ws/push?n=1&close=none', '/ws/deflate']) {
      const c = client(path);
      await c.opened;
      const t0 = performance.now();
      c.ws.close(1000, 'bye');
      const e = await c.closed;
      const ms = performance.now() - t0;
      assert(ms < 500, `${path}: ${ms.toFixed(0)} ms`);
      eq([e.code, e.reason, e.wasClean], [1000, 'bye', true], path);
    }
  });

  await check('15 a frame header of 2^40 bytes: 1009, and nothing allocated for it', async () => {
    const before = process.memoryUsage().arrayBuffers;
    const r = await raw('/ws/echo');
    const head = Buffer.alloc(14);
    head[0] = 0x82;
    head[1] = 0x80 | 127;
    head.writeUInt32BE(2 ** 8, 2);     // 2^40
    head.writeUInt32BE(0, 6);
    r.write(head);
    r.write(Buffer.alloc(1000));
    eq(closeCode(await r.frame(1)), 1009, 'close code');
    assert(process.memoryUsage().arrayBuffers - before < 50e6, 'memory grew');
    const s = await raw('/ws/echo');
    s.send(2, Buffer.alloc(10), { fin: false });
    const more = Buffer.alloc(14);
    more[0] = 0x00;
    more[1] = 0x80 | 127;
    more.writeUInt32BE(0, 2);
    more.writeUInt32BE(256 * 1024 * 1024, 6);
    s.write(more);
    eq(closeCode(await s.frame(1)), 1009, 'a message over 256 MiB');
  });

  await check('16 RSV1 without an extension, a ping of 126 bytes, a fragmented ping, a continuation first: 1002', async () => {
    for (const [what, go] of [
      ['RSV1', r => r.send(1, Buffer.from('x'), { rsv: 4 })],
      ['RSV2', r => r.send(1, Buffer.from('x'), { rsv: 2 })],
      ['RSV3', r => r.send(1, Buffer.from('x'), { rsv: 1 })],
      ['a ping of 126 bytes', r => r.send(9, Buffer.alloc(126))],
      ['a fragmented ping', r => r.send(9, Buffer.from('x'), { fin: false })],
      ['a continuation first', r => r.send(0, Buffer.from('x'))],
      ['a new message inside one', r => { r.send(1, Buffer.from('x'), { fin: false }); r.send(1, Buffer.from('y')); }],
      ['opcode 3', r => r.send(3, Buffer.from('x'))],
    ]) {
      const r = await raw('/ws/echo');
      go(r);
      eq(closeCode(await r.frame(1)), 1002, what);
      r.socket.destroy();
    }
  });

  await check('17 /ws/deflate: permessage-deflate accepted, a compressed frame inflated, echoed uncompressed', async () => {
    const c = client('/ws/deflate');
    await c.opened;
    assert(/^permessage-deflate/.test(c.ws.extensions), 'extensions ' + JSON.stringify(c.ws.extensions));
    c.ws.send('compress me, compress me, compress me');
    eq(await c.next(), 'compress me, compress me, compress me', 'echo');
    eq(last('/ws/deflate').deflate, true, 'log');
    c.ws.close();
    await c.closed;
    const plain = client('/ws/echo');
    await plain.opened;
    eq(plain.ws.extensions, '', 'none on /ws/echo');
    plain.ws.close();
    const r = await raw('/ws/deflate', { headers: { 'Sec-WebSocket-Extensions': 'permessage-deflate' } });
    assert(/^permessage-deflate/.test(r.headers['sec-websocket-extensions'] || ''), 'raw: ' + r.headers['sec-websocket-extensions']);
    const deflated = zlib.deflateRawSync(Buffer.from('caffè caffè caffè', 'utf8'), { finishFlush: zlib.constants.Z_SYNC_FLUSH });
    r.send(1, deflated.subarray(0, deflated.length - 4), { rsv: 4 });
    const f = await r.frame(1);
    eq([f.opcode, f.rsv, f.payload.toString('utf8')], [1, 0, 'caffè caffè caffè'], 'inflated');
    r.socket.destroy();
    await plain.closed;
  });

  await check('18 /ws/push?bom=1: the server sends EF BB BF, and node strips it', async () => {
    const r = await raw('/ws/push?n=1&bom=1');
    const f = await r.frame(1);
    eq([...f.payload], [0xef, 0xbb, 0xbf, 0x31], 'the bytes of the server');
    r.socket.destroy();
    const c = client('/ws/push?n=2&bom=1&size=8');
    const first = await c.next();
    eq([first, await c.next()], ['1 xxxxxx', '2 xxxxxx'], 'what node delivers (as measured with node 24 and 25)');
    eq(bytesOf(first).length + 3, pushMessage(1, { size: 8, bom: true }).length, 'three bytes fewer');
    await c.closed;
  });

  await check('19 /ws/hang on the second port never answers, and close() ends it', async () => {
    assert(srv.hangPort && srv.hangPort !== srv.port, 'hangPort ' + srv.hangPort);
    const r = await raw('/ws/hang', { port: srv.hangPort, handshake: false });
    await sleep(1000);
    eq([r.status, r.ended], [null, false], 'no answer in 1 s');
    assert(srv.ws.some(e => e.path === '/ws/hang' && e.hang), 'logged');
    const c = client('/ws/echo');
    await c.opened;
    c.ws.close();
    await c.closed;
    r.socket.destroy();
  });
} finally {
  await srv.close();
}

console.log(`${passes} passed, ${failures} failed`);
if (failures > 0) process.exit(1);
