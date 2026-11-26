// websocket-linger.mjs [CHECKOUT] - web-socket.js in node's vm against the /ws/hang
// listener of the test server: 40 sockets opened with a 200 ms handshake timeout
// and released, then how many linger (count toward the 32 open) and for how long.
// It found the leak fixed in 59a08b059 (a socket closed while connecting lingered
// for ever); now it prints 40 opened, 0 refused, 0 lingering.  CHECKOUT is a
// checkout with web-socket.js (default: this one).   node websocket-linger.mjs
import fs from 'node:fs';
import vm from 'node:vm';
const wt = process.argv[2] || new URL('../../..', import.meta.url).pathname.replace(/\/$/, '');
const { startFetchHttp } = await import(wt + '/tests/wasm/lib/fetch-http.mjs');
const src = fs.readFileSync(wt + '/packaging/emscripten/host/web-socket.js', 'utf8');
const ctx = { console, setTimeout, clearTimeout, TextEncoder, TextDecoder, URL, Promise, Uint8Array, Map, Set, Math, String, JSON, process };
vm.createContext(ctx);
vm.runInContext(src + '\nthis.PharoWebSocket = PharoWebSocket;', ctx);
const srv = await startFetchHttp({});
const svc = ctx.PharoWebSocket.create({ signal: () => {}, WebSocket: () => globalThis.WebSocket });
const enc = new TextEncoder();
const sleep = ms => new Promise(r => setTimeout(r, ms));
let sem = 1, opened = 0, refused = 0;
// the image: connect with a 200 ms handshake timeout, then settle and release (as WebFetchWebSocket does)
for (let i = 0; i < 40; i++) {
  const s = sem; sem += 3;
  const id = svc.open(s, s + 1, s + 2, enc.encode(`ws://127.0.0.1:${srv.hangPort}/ws/hang`), 1 << 20, 1 << 20, 200);
  if (!id) { refused++; continue; }
  opened++;
  while ((svc.state(id) & 7) < 4) await sleep(20);
  svc.release(id);
}
console.log('opened', opened, 'refused', refused, 'lingering', svc.lingering());
const t0 = Date.now();
while (svc.lingering() > 0 && Date.now() - t0 < 15000) await sleep(1000);
console.log('lingering after', Date.now() - t0, 'ms:', svc.lingering(), 'hang sockets at server', srv.ws.filter(e => e.hang).length);
process.exit(0);
