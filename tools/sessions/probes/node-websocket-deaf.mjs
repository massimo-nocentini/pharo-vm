// node-websocket-deaf.mjs - a child node opens a WebSocket to a server that
// answers the handshake and ignores everything after it, sends and closes:
// how long the child lives with that closing socket (node keeps running until
// TCP ends), the Origin and extensions node sends.  The measure behind the exit
// guard of node-pump.js (docs/WebSocket.md, In node).   node node-websocket-deaf.mjs
import http from 'node:http'; import crypto from 'node:crypto'; import { spawn } from 'node:child_process';
const s = http.createServer(); const socks = [];
s.on('upgrade', (q, so) => { socks.push(so); const k = crypto.createHash('sha1').update(q.headers['sec-websocket-key'] + '258EAFA5-E914-47DA-95CA-C5AB0DC85B11').digest('base64');
  so.write(`HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: ${k}\r\n\r\n`); so.on('data', () => {}); so.on('error', () => {});
  console.log('origin header:', JSON.stringify(q.headers.origin), 'ext:', JSON.stringify(q.headers['sec-websocket-extensions'])); });
await new Promise(f => s.listen(0, '127.0.0.1', f));
const port = s.address().port, t0 = Date.now();
const child = spawn(process.execPath, ['-e', `const w = new WebSocket('ws://127.0.0.1:${port}/'); w.onopen = () => { w.send('x'); console.log('buffered after send', w.bufferedAmount); w.close(1000, 'bye'); console.log('closed asked'); }; w.onclose = e => console.log('close event', e.code, e.wasClean, Date.now());`], { stdio: 'inherit' });
child.on('exit', c => { console.log('child exit', c, 'after', Date.now() - t0, 'ms'); socks.forEach(x => x.destroy()); s.close(); });
setTimeout(() => { console.log('child still alive after 8 s'); child.kill(); }, 8000).unref();
