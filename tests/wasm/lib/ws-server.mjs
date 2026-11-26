// ws-server.mjs - the WebSocket endpoints of the test server (RFC 6455), without dependencies
//
//   import { attachWebSocket, startHangServer } from './ws-server.mjs';
//   const ws = attachWebSocket(httpServer, { log });     // { sockets, rooms, close() }
//   const hang = await startHangServer({ host, log });   // { port, server, sockets, close() }
//
// lib/fetch-http.mjs attaches it to its HTTP server, so that one URL serves
// both, and starts the second listener; its /_stats lists the connections in
// `ws` and its /_reset clears them.  A connection's entry in log:
//
//   { path, url, offered: [subprotocols], protocol, extensions (offered),
//     deflate (accepted), origin, cookie (whether one came), frames,
//     messages, bytes (of the messages), pongs: [payloads], code, reason
//     (of the close frame of the client, null before it), sentCode (of the
//     server's), clean (both close frames went before TCP ended), ended }
//
// The handshake: GET, Upgrade: websocket, version 13 (426 otherwise), a key
// of 16 bytes (400 without), the first offered subprotocol that the endpoint
// takes, no extension but on /ws/deflate.  Frames of 7-, 16- and 64-bit
// lengths; a frame or a message over 256 MiB is closed with 1009 before
// anything is allocated; an unmasked client frame, RSV bits without an
// extension, an unknown opcode, a control frame over 125 bytes or
// fragmented, a continuation out of place: 1002; a text message that is not
// UTF-8 once joined: 1007.  Pings are answered, control frames may come
// between fragments.  A close is answered with the same code and reason, and
// then the server ends TCP (RFC 6455, 7.1.1), on every endpoint but
// /ws/deaf; after a close of its own it ends TCP when the client answers, or
// after 2 s, and at once after a failure of the client.  Server frames are not masked.
//
// Endpoints (paths of the HTTP server; what is not said is echoed after it):
//
//   /ws/echo                 each message back with its type; takes the
//                            subprotocols echo and chat
//   /ws/push?n=3&size=&type=text&interval=0&close=1000&reason=&bom=0
//                            n messages, then a close with that code
//                            (close=none: none); message i (from 1) is, as
//                            text, String(i), padded with x to size bytes
//                            after a blank, preceded by U+FEFF with bom=1;
//                            as binary, size (4) bytes (i + j) & 255
//   /ws/broadcast?room=      each message to every socket of the room, the
//                            sender too
//   /ws/close?code=1000&reason=&after=0&abrupt=0
//                            a close after `after` ms; abrupt=1 ends TCP
//                            without a close frame
//   /ws/fragmented?parts=3&type=text
//                            one message in `parts` frames, a ping between
//                            the first two: as text 'fragmented:' and
//                            ' café ✓' parts times, cut in equal byte
//                            counts (inside the characters); as binary
//                            16 * parts bytes j & 255
//   /ws/ping?payload=ping    a ping; the pong's payload goes to pongs
//   /ws/big?size=1048576&type=binary
//                            one message of size bytes: j & 255, or x
//   /ws/flood?n=1000&size=1024&type=binary
//                            n messages of push at once
//   /ws/slow-read?resume=3000
//                            does not read TCP (pause()) for resume ms
//                            (0: never), so that the client's
//                            bufferedAmount grows; then counts and drops
//   /ws/deaf                 answers the handshake, then ignores everything
//                            and never closes
//   /ws/deflate              accepts permessage-deflate (no context
//                            takeover either way), inflates, echoes
//                            uncompressed
//   /ws/refuse?status=403    answers the upgrade with that status
//
// The second listener (startHangServer) never answers anything: /ws/hang.
// Chromium makes one handshake at a time per address and port, so a hanging
// one on the main port would hold back the others (websocket.spec.mjs
// measures it); on a port of its own it holds back none of them.

import crypto from 'node:crypto';
import http from 'node:http';
import zlib from 'node:zlib';
import { isUtf8 } from 'node:buffer';

export const MAX_FRAME = 256 * 1024 * 1024;
export const MAX_LOG = 5000;
const GUID = '258EAFA5-E914-47DA-95CA-C5AB0DC85B11';
const TOKEN = /^[!#$%&'*+\-.^_`|~0-9A-Za-z]+$/;
const TAIL = Buffer.from([0x00, 0x00, 0xff, 0xff]);
const PROTOCOLS = { '/ws/echo': ['echo', 'chat'] };
const ENDPOINTS = new Set(['/ws/echo', '/ws/push', '/ws/broadcast', '/ws/close', '/ws/fragmented', '/ws/ping', '/ws/big',
  '/ws/flood', '/ws/slow-read', '/ws/deaf', '/ws/deflate', '/ws/refuse']);

export const acceptOf = key => crypto.createHash('sha1').update(key + GUID).digest('base64');

// A frame of the server: FIN, the opcode, the payload, unmasked
export function frame(opcode, payload = Buffer.alloc(0), { fin = true, rsv = 0, mask = null } = {}) {
  const n = payload.length;
  const head = n < 126 ? 2 : n < 65536 ? 4 : 10;
  const out = Buffer.alloc(head + (mask ? 4 : 0) + n);
  out[0] = (fin ? 0x80 : 0) | (rsv << 4) | opcode;
  if (n < 126) out[1] = n;
  else if (n < 65536) { out[1] = 126; out.writeUInt16BE(n, 2); }
  else { out[1] = 127; out.writeUInt32BE(Math.floor(n / 2 ** 32), 2); out.writeUInt32BE(n >>> 0, 6); }
  if (mask) {
    out[1] |= 0x80;
    mask.copy(out, head);
    for (let i = 0; i < n; i++) out[head + 4 + i] = payload[i] ^ mask[i & 3];
  } else payload.copy(out, head);
  return out;
}

const closePayload = (code, reason = '') => {
  if (code === undefined || code === null) return Buffer.alloc(0);
  const r = Buffer.from(reason, 'utf8');
  const b = Buffer.alloc(2 + r.length);
  b.writeUInt16BE(code, 0);
  r.copy(b, 2);
  return b;
};

// message i of push and flood
export function pushMessage(i, { type = 'text', size = 0, bom = false } = {}) {
  if (type === 'binary') {
    const b = Buffer.alloc(size || 4);
    for (let j = 0; j < b.length; j++) b[j] = (i + j) & 255;
    return b;
  }
  let s = String(i);
  if (size > s.length) s = (s + ' ').padEnd(size, 'x');
  return Buffer.from((bom ? '﻿' : '') + s, 'utf8');
}

// the message of /ws/fragmented
export function fragmentedMessage(parts, type = 'text') {
  if (type === 'binary') {
    const b = Buffer.alloc(16 * parts);
    for (let j = 0; j < b.length; j++) b[j] = j & 255;
    return b;
  }
  return Buffer.from('fragmented:' + ' café ✓'.repeat(parts), 'utf8');
}

function bigMessage(size, type) {
  if (type === 'text') return Buffer.alloc(size, 'x');
  const b = Buffer.allocUnsafe(size);
  const block = Buffer.alloc(Math.min(size, 65536));
  for (let j = 0; j < block.length; j++) block[j] = j & 255;
  for (let at = 0; at < size; at += block.length) block.copy(b, at, 0, Math.min(block.length, size - at));
  return b;
}

function refuse(socket, status, extra = '') {
  socket.end(`HTTP/1.1 ${status} ${http.STATUS_CODES[status] || 'Refused'}\r\nContent-Length: 0\r\nConnection: close\r\n${extra}\r\n`);
}

const keep = (log, entry) => {
  log.push(entry);
  if (log.length > MAX_LOG) log.splice(0, log.length - MAX_LOG);
};

export function attachWebSocket(server, { log = [] } = {}) {
  const sockets = new Set();
  const rooms = new Map();
  const timers = new Set();
  const later = (f, ms) => {
    const t = setTimeout(() => { timers.delete(t); f(); }, ms);
    timers.add(t);
    return t;
  };

  server.on('upgrade', (req, socket, head) => {
    socket.on('error', () => {});
    const parsed = new URL(req.url, 'http://localhost');
    const path = parsed.pathname, q = parsed.searchParams;
    const number = (name, value) => q.has(name) ? Number(q.get(name)) : value;
    if (!ENDPOINTS.has(path)) { refuse(socket, 404); return; }
    const key = req.headers['sec-websocket-key'];
    if (req.method !== 'GET' || !/\bwebsocket\b/i.test(req.headers.upgrade || '')) { refuse(socket, 400); return; }
    if (req.headers['sec-websocket-version'] !== '13') { refuse(socket, 426, 'Sec-WebSocket-Version: 13\r\n'); return; }
    if (!key || Buffer.from(key, 'base64').length !== 16) { refuse(socket, 400); return; }
    const offered = (req.headers['sec-websocket-protocol'] || '').split(',').map(s => s.trim()).filter(s => s);
    const entry = {
      path, url: req.url, offered, protocol: '', extensions: req.headers['sec-websocket-extensions'] || '',
      deflate: false, origin: req.headers.origin || null, cookie: !!req.headers.cookie,
      frames: 0, messages: 0, bytes: 0, pongs: [], code: null, reason: null, sentCode: null, clean: false, ended: false,
    };
    keep(log, entry);
    if (path === '/ws/refuse') { refuse(socket, number('status', 403)); return; }
    if (offered.some(p => !TOKEN.test(p))) { refuse(socket, 400); return; }
    entry.protocol = offered.find(p => (PROTOCOLS[path] || []).includes(p)) || '';
    entry.deflate = path === '/ws/deflate' && /(^|,)\s*permessage-deflate\b/.test(entry.extensions);
    const lines = ['HTTP/1.1 101 Switching Protocols', 'Upgrade: websocket', 'Connection: Upgrade',
      'Sec-WebSocket-Accept: ' + acceptOf(key)];
    if (entry.protocol) lines.push('Sec-WebSocket-Protocol: ' + entry.protocol);
    if (entry.deflate) lines.push('Sec-WebSocket-Extensions: permessage-deflate; server_no_context_takeover; client_no_context_takeover');
    socket.write(lines.join('\r\n') + '\r\n\r\n');
    socket.setNoDelay(true);
    sockets.add(socket);

    const own = new Set();
    const wait = (f, ms) => { const t = later(() => { own.delete(t); f(); }, ms); own.add(t); };
    let closeSent = false, closeGot = false, gone = false;
    const room = path === '/ws/broadcast' ? q.get('room') || '' : null;
    const conn = {
      send(opcode, payload, options) { if (!gone && !closeSent) socket.write(frame(opcode, payload, options)); },
      // end: TCP at once, as after a failure of the client
      close(code, reason = '', end = false) {
        if (gone || closeSent) return;
        closeSent = true;
        entry.sentCode = code === undefined ? null : code;
        socket.write(frame(8, closePayload(code, reason)));
        if (closeGot || end) socket.end();
        else wait(() => socket.destroy(), 2000);
      },
    };
    if (room !== null) {
      if (!rooms.has(room)) rooms.set(room, new Set());
      rooms.get(room).add(conn);
    }
    // the client ended TCP: end it too (the sockets of the HTTP server are
    // half-open ones, which would stay so), /ws/deaf included
    socket.on('end', () => socket.end());
    socket.on('close', () => {
      gone = true;
      entry.ended = true;
      entry.clean = closeSent && closeGot;
      sockets.delete(socket);
      for (const t of own) { clearTimeout(t); timers.delete(t); }
      if (room !== null) rooms.get(room).delete(conn);
    });
    const fail = code => { parser.stop(); conn.close(code, '', true); };

    // what an endpoint does with a whole message of the client
    const onMessage = (opcode, payload) => {
      entry.messages++;
      entry.bytes += payload.length;
      if (path === '/ws/slow-read' || path === '/ws/deaf') return;
      if (room !== null) { for (const c of rooms.get(room)) c.send(opcode, payload); return; }
      conn.send(opcode, payload);
    };
    const onControl = (opcode, payload) => {
      if (opcode === 9) conn.send(10, payload);
      else if (opcode === 10) entry.pongs.push(payload.toString('utf8'));
      else if (opcode === 8) {
        if (payload.length === 1) return fail(1002);
        const code = payload.length >= 2 ? payload.readUInt16BE(0) : null;
        const reason = payload.subarray(2);
        if (!isUtf8(reason)) return fail(1007);
        closeGot = true;
        entry.code = code;
        entry.reason = reason.toString('utf8');
        parser.stop();
        if (closeSent) socket.end();
        else conn.close(code === null ? undefined : code, entry.reason);
      }
    };
    const parser = makeParser({
      deflate: entry.deflate,
      onFrame: () => { entry.frames++; },
      onMessage: (opcode, payload) => {
        if (opcode === 1 && !isUtf8(payload)) return fail(1007);
        onMessage(opcode, payload);
      },
      onControl,
      onError: code => fail(code),
    });
    if (path === '/ws/deaf') {
      socket.on('data', () => {});
      return;
    }
    socket.on('data', d => parser.push(d));
    if (head && head.length) parser.push(head);

    const type = q.get('type') === 'binary' ? 'binary' : q.get('type') === 'text' ? 'text' : null;
    switch (path) {
      case '/ws/push': {
        const n = number('n', 3), interval = number('interval', 0), size = number('size', 0), bom = q.get('bom') === '1';
        const t = type || 'text', opcode = t === 'binary' ? 2 : 1;
        const end = () => { if (q.get('close') !== 'none') conn.close(number('close', 1000), q.get('reason') || ''); };
        const send = i => {
          if (i > n) return end();
          conn.send(opcode, pushMessage(i, { type: t, size, bom }));
          if (interval > 0) wait(() => send(i + 1), interval); else send(i + 1);
        };
        send(1);
        break;
      }
      case '/ws/flood': {
        const n = number('n', 1000), size = number('size', 1024), t = type || 'binary';
        for (let i = 1; i <= n; i++) conn.send(t === 'binary' ? 2 : 1, pushMessage(i, { type: t, size }));
        break;
      }
      case '/ws/close': {
        const go = () => {
          if (q.get('abrupt') === '1') socket.destroy();
          else conn.close(number('code', 1000), q.get('reason') || '');
        };
        const after = number('after', 0);
        if (after > 0) wait(go, after); else go();
        break;
      }
      case '/ws/fragmented': {
        const parts = Math.max(1, number('parts', 3)), t = type || 'text';
        const bytes = fragmentedMessage(parts, t), step = Math.ceil(bytes.length / parts);
        for (let j = 0; j < parts; j++) {
          conn.send(j === 0 ? (t === 'binary' ? 2 : 1) : 0, bytes.subarray(j * step, Math.min(bytes.length, (j + 1) * step)),
            { fin: j === parts - 1 });
          if (j === 0 && parts > 1) conn.send(9, Buffer.from('between'));
        }
        break;
      }
      case '/ws/ping':
        conn.send(9, Buffer.from(q.get('payload') || 'ping', 'utf8'));
        break;
      case '/ws/big': {
        const t = type || 'binary';
        conn.send(t === 'binary' ? 2 : 1, bigMessage(number('size', 1 << 20), t));
        break;
      }
      case '/ws/slow-read': {
        socket.pause();
        const resume = number('resume', 3000);
        if (resume > 0) wait(() => socket.resume(), resume);
        break;
      }
    }
  });

  return {
    sockets, rooms, log,
    // ends every upgraded socket: they are not the HTTP server's any more,
    // and server.close() would wait for them
    close() {
      for (const t of timers) clearTimeout(t);
      timers.clear();
      for (const s of sockets) s.destroy();
      sockets.clear();
    },
  };
}

// The frames of a client: masked, of at most MAX_FRAME bytes
function makeParser({ deflate, onFrame, onMessage, onControl, onError }) {
  const chunks = [];
  let have = 0, stopped = false;
  let message = null;           // {opcode, parts, size, compressed}
  const bytesOf = n => {
    // the first n bytes of what came, removed
    const out = Buffer.allocUnsafe(n);
    let at = 0;
    while (at < n) {
      const c = chunks[0], k = Math.min(c.length, n - at);
      c.copy(out, at, 0, k);
      at += k;
      if (k === c.length) chunks.shift(); else chunks[0] = c.subarray(k);
    }
    have -= n;
    return out;
  };
  const peek = n => {
    if (chunks[0].length >= n) return chunks[0];
    const b = bytesOf(Math.min(have, 14));
    chunks.unshift(b);
    have += b.length;
    return b;
  };
  const step = () => {
    if (have < 2) return false;
    let h = peek(Math.min(have, 14));
    const b0 = h[0], b1 = h[1];
    const fin = (b0 & 0x80) !== 0, rsv = (b0 >> 4) & 7, opcode = b0 & 15, masked = (b1 & 0x80) !== 0;
    let len = b1 & 127, at = 2;
    if (len === 126) {
      if (have < 4) return false;
      len = h.readUInt16BE(2);
      at = 4;
    } else if (len === 127) {
      if (have < 10) return false;
      const high = h.readUInt32BE(2), low = h.readUInt32BE(6);
      if (high >= 0x80000000) { onError(1002); return false; }
      len = high * 2 ** 32 + low;
      at = 10;
    }
    if (!masked) { onError(1002); return false; }
    const control = opcode >= 8;
    if ((opcode > 2 && opcode < 8) || opcode > 10) { onError(1002); return false; }
    if (control && (len > 125 || !fin)) { onError(1002); return false; }
    // RSV1 (4) is the compressed bit of permessage-deflate, on the first frame of a message
    if (rsv & 3 || (rsv & 4 && (!deflate || control || opcode === 0))) { onError(1002); return false; }
    if (len > MAX_FRAME || (!control && message && message.size + len > MAX_FRAME)) { onError(1009); return false; }
    if (!control && (opcode === 0) === !message) { onError(1002); return false; }
    if (have < at + 4 + len) return false;
    bytesOf(at);
    const mask = bytesOf(4);
    const payload = bytesOf(len);
    for (let i = 0; i < len; i++) payload[i] ^= mask[i & 3];
    onFrame();
    if (control) { onControl(opcode, payload); return !stopped; }
    if (!message) message = { opcode, parts: [], size: 0, compressed: (rsv & 4) !== 0 };
    message.parts.push(payload);
    message.size += len;
    if (fin) {
      const m = message;
      message = null;
      let data = Buffer.concat(m.parts, m.size);
      if (m.compressed) {
        try { data = zlib.inflateRawSync(Buffer.concat([data, TAIL]), { finishFlush: zlib.constants.Z_SYNC_FLUSH, maxOutputLength: MAX_FRAME }); }
        catch (e) { onError(e && e.code === 'ERR_BUFFER_TOO_LARGE' ? 1009 : 1007); return false; }
      }
      onMessage(m.opcode, data);
    }
    return !stopped;
  };
  return {
    push(d) {
      if (stopped || !d.length) return;
      chunks.push(d);
      have += d.length;
      while (!stopped && step()) { /* the next frame */ }
    },
    stop() { stopped = true; chunks.length = 0; have = 0; },
  };
}

// The second listener: holds every connection and answers nothing
export async function startHangServer({ host = '127.0.0.1', port = 0, log = [] } = {}) {
  const sockets = new Set();
  const server = http.createServer(req => {
    req.on('error', () => {});
  });
  const hold = socket => {
    sockets.add(socket);
    socket.on('error', () => {});
    socket.on('close', () => sockets.delete(socket));
  };
  server.on('connection', hold);
  server.on('upgrade', (req, socket) => {
    hold(socket);
    keep(log, { path: new URL(req.url, 'http://localhost').pathname, url: req.url, hang: true,
      offered: [], protocol: '', origin: req.headers.origin || null, ended: false });
  });
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(port, host, () => { server.removeListener('error', reject); resolve(); });
  });
  return {
    port: server.address().port,
    server,
    sockets,
    close() {
      for (const s of sockets) s.destroy();
      sockets.clear();
      return new Promise(resolve => server.close(() => resolve()));
    },
  };
}
