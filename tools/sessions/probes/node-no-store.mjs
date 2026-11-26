// node-no-store.mjs - what node's fetch sends with cache: 'no-store' (it adds
// Cache-Control: no-cache and Pragma: no-cache by itself), the measure behind
// flag 2 of primitiveFetchStart and the live test that checks the flag reached
// fetch (docs/WebFetch.md).   node node-no-store.mjs
import http from 'node:http';
const srv = http.createServer((req, res) => { res.writeHead(200, {'content-type':'text/event-stream','cache-control':'max-age=60'}); res.end('data: ' + req.headers['cache-control'] + ' ' + req.headers['pragma'] + '\n\n'); });
await new Promise(f => srv.listen(0, '127.0.0.1', f));
const url = `http://127.0.0.1:${srv.address().port}/s`;
for (const cache of ['default', 'no-store']) {
  try { const r = await fetch(url, { cache, mode: 'cors', credentials: 'omit', redirect: 'manual' }); console.log(cache, r.status, JSON.stringify(await r.text())); }
  catch (e) { console.log(cache, 'threw', e.message, e.cause && e.cause.message); }
}
console.log(process.version);
srv.close();
