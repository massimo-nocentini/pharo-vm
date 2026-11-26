// browser-websocket-cookies.mjs [CHECKOUT] - which cookies of localhost Chromium sends
// with a WebSocket handshake from a page and from a worker of another site
// (127.0.0.1), for SameSite None/Lax/Strict, secure or not: the measure behind
// the refusal of the hosts of the site's domain (docs/WebSocket.md, Security).
// CHECKOUT has tests/wasm/lib/fetch-http.mjs with /ws/echo (default: this one).
//   PLAYWRIGHT_MODULE=... PLAYWRIGHT_BROWSERS_PATH=... node browser-websocket-cookies.mjs
import { createRequire } from 'node:module';
import http from 'node:http';
const require = createRequire(import.meta.url);
const pw = require(process.env.PLAYWRIGHT_MODULE);
const { startFetchHttp } = await import((process.argv[2] || new URL('../../..', import.meta.url).pathname) + '/tests/wasm/lib/fetch-http.mjs');
const web = await startFetchHttp();
const site = http.createServer((q, r) => { r.writeHead(200, {'Content-Type': 'text/html'}); r.end('<p>x'); });
await new Promise(f => site.listen(0, '127.0.0.1', f));
const b = await pw.chromium.launch();
console.log('version', b.version());
for (const [name, opts, host] of [['https-none', { sameSite: 'None', secure: true, url: undefined, domain: 'localhost', path: '/' }, 'localhost'], ['https-url', { sameSite: 'None', secure: true, url: 'https://localhost/' }, 'localhost'], ['none-secure', { sameSite: 'None', secure: true }, 'localhost'], ['lax', { sameSite: 'Lax' }, 'localhost'], ['none-insecure', { sameSite: 'None', secure: false }, 'localhost'], ['strict', { sameSite: 'Strict' }, 'localhost']]) {
  const c = await b.newContext();
  const ck = { name: 'c' + name, value: '1', url: `http://${host}:${web.port}/`, ...opts }; if (!ck.url) delete ck.url;
  try { await c.addCookies([ck]); } catch (e) { console.log(name, 'addCookies', e.message.split('\n')[0]); }
  console.log(name, JSON.stringify(await c.cookies()));
  const p = await c.newPage();
  await p.goto(`http://127.0.0.1:${site.address().port}/`);
  for (const where of ['page', 'worker']) {
    const tag = name + '-' + where;
    const src = `new Promise(f => { const w = new WebSocket('ws://${host}:${web.port}/ws/echo?t=${tag}'); w.onopen = () => { w.close(); f(1); }; w.onerror = () => f(0); })`;
    if (where === 'page') await p.evaluate(src);
    else await p.evaluate(s => new Promise(f => { const w = new Worker(URL.createObjectURL(new Blob([`${s}.then(v => postMessage(v))`]))); w.onmessage = e => f(e.data); }), src);
    await new Promise(f => setTimeout(f, 200));
    const e = web.ws.find(e => e.url.includes('t=' + tag));
    console.log(tag, e && e.cookie);
  }
  await c.close();
}
await b.close(); site.close(); await web.close();
