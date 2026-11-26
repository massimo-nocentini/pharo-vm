// node-websocket-close.mjs - node's WebSocket against a server that sends a
// text message with a BOM, then a close frame, and keeps TCP open: readyState
// goes to 2 with no close event, and a send then neither throws nor buffers;
// plus what a fatal TextDecoder does with a BOM.  The measures behind the close
// timer of web-socket.js and its ignoreBOM (docs/WebSocket.md, In node).
//   node node-websocket-close.mjs   (node 22 or later: a global WebSocket)
import http from 'node:http'; import crypto from 'node:crypto';
const s = http.createServer();
s.on('upgrade', (q, so) => {
  const k = crypto.createHash('sha1').update(q.headers['sec-websocket-key'] + '258EAFA5-E914-47DA-95CA-C5AB0DC85B11').digest('base64');
  so.write(`HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: ${k}\r\n\r\n`);
  so.on('error', () => {});
  // text frame with BOM, then a close frame 1000 'x'; keep TCP open
  so.write(Buffer.from([0x81, 4, 0xEF, 0xBB, 0xBF, 0x41]));
  setTimeout(() => so.write(Buffer.from([0x88, 3, 0x03, 0xE8, 0x78])), 100);
  so.on('data', d => console.log('server got', d.length, 'bytes, opcode', d[0] & 15));
});
await new Promise(f => s.listen(0, '127.0.0.1', f));
const w = new WebSocket(`ws://127.0.0.1:${s.address().port}/`);
const t0 = Date.now();
w.onmessage = e => console.log('message', JSON.stringify(e.data), e.data.length);
w.onclose = e => { console.log('close', e.code, e.wasClean, Date.now() - t0); process.exit(0); };
w.onerror = e => console.log('error', e.message);
let last = -1;
const iv = setInterval(() => { if (w.readyState !== last) { last = w.readyState; console.log('readyState', last, Date.now() - t0); } }, 10);
setTimeout(() => { console.log('no close event after 6 s; send returns', (() => { try { w.send('late'); return 'ok, buffered ' + w.bufferedAmount; } catch (e) { return e.name; } })()); process.exit(0); }, 6000);
console.log('fatal decoder BOM:', new TextDecoder('utf-8', {fatal: true}).decode(new Uint8Array([0xEF,0xBB,0xBF,0x41])).length, 'ignoreBOM:', new TextDecoder('utf-8', {fatal: true, ignoreBOM: true}).decode(new Uint8Array([0xEF,0xBB,0xBF,0x41])).length);
