// browser-cache-replay.mjs - whether Chromium's HTTP cache replays an event
// stream to a fetch of a worker (as vm-worker.js makes them): for each of
// Cache-Control max-age=60, a Last-Modified only, and no-cache, three fetches
// of one URL: default, default with Last-Event-ID and Cache-Control: no-cache,
// and no-store; 'hit 1' twice means a replay.  The measure behind the cache mode
// no-store of event sources (docs/WebFetch.md, In a browser).
//   PLAYWRIGHT_MODULE=<.../node_modules/playwright> PLAYWRIGHT_BROWSERS_PATH=... node browser-cache-replay.mjs
import http from 'node:http';
import { createRequire } from 'node:module';
const require = createRequire(import.meta.url);
const { chromium } = require(process.env.PLAYWRIGHT_MODULE);
let hits = {};
const api = http.createServer((req, res) => {
  const u = new URL(req.url, 'http://x');
  const key = u.pathname + u.search;
  hits[key] = (hits[key] || 0) + 1;
  if (req.method === 'OPTIONS') { res.writeHead(204, { 'Access-Control-Allow-Origin': '*', 'Access-Control-Allow-Headers': '*' }); return res.end(); }
  const cc = u.searchParams.get('cc');
  const h = { 'Content-Type': 'text/event-stream', 'Access-Control-Allow-Origin': '*' };
  if (cc) h['Cache-Control'] = cc;
  if (u.searchParams.get('lm')) h['Last-Modified'] = new Date(Date.now() - 86400000).toUTCString();
  res.writeHead(200, h); res.flushHeaders();
  res.write('data: hit ' + hits[key] + '\n\n');
  if (u.searchParams.get('end')) res.end(); // else hang
});
await new Promise(r => api.listen(0, '127.0.0.1', r));
const B = `http://127.0.0.1:${api.address().port}`;
const page0 = http.createServer((req, res) => { res.writeHead(200, { 'Content-Type': 'text/html' }); res.end('<!doctype html><title>t</title>'); });
await new Promise(r => page0.listen(0, '127.0.0.1', r));
const browser = await chromium.launch();
const page = await browser.newPage();
await page.goto(`http://127.0.0.1:${page0.address().port}/`);
// in a worker, as vm-worker.js does
const run = (src) => page.evaluate(src => new Promise(res => { const w = new Worker(URL.createObjectURL(new Blob([src]))); w.onmessage = e => res(e.data); }), src);
for (const [cc, lm] of [['max-age=60', ''], ['', '1'], ['no-cache', '']]) {
  const url = `${B}/replay?cc=${cc}&lm=${lm}&end=1&t=${Math.random()}`;
  const out = await run(`(async () => {
    const a = await (await fetch(${JSON.stringify(url)}, { mode: 'cors', credentials: 'omit', cache: 'default' })).text();
    const b = await (await fetch(${JSON.stringify(url)}, { mode: 'cors', credentials: 'omit', cache: 'default', headers: { 'Last-Event-ID': '1', 'Cache-Control': 'no-cache' } })).text();
    const c = await (await fetch(${JSON.stringify(url)}, { mode: 'cors', credentials: 'omit', cache: 'no-store' })).text();
    postMessage(JSON.stringify([a, b, c]));
  })()`);
  console.log(`replay, Cache-Control '${cc}' Last-Modified '${lm}':`, out);
}
await browser.close(); api.close(); page0.close();
