// node-header-trim.mjs - why web-fetch.js trims header values of spaces and
// tabs only: the UTF-8 of 'città' read as Latin-1 ends in U+00A0, which
// String.prototype.trim() removes, so a Last-Event-ID lost its last byte
// (docs/WebFetch.md, the request).   node node-header-trim.mjs
import http from 'node:http';
const srv = http.createServer((req, res) => { res.end(JSON.stringify([...Buffer.from(req.headers['last-event-id'] || '', 'latin1')])); });
await new Promise(r => srv.listen(0, '127.0.0.1', r));
const url = `http://127.0.0.1:${srv.address().port}/`;
const id = 'città';
const asLatin1 = Buffer.from(id, 'utf8').toString('latin1');
console.log('utf8 bytes', [...Buffer.from(id, 'utf8')]);
console.log('after trim()', [...Buffer.from(asLatin1.trim(), 'latin1')]);
let r = await fetch(url, { headers: [['Last-Event-ID', asLatin1]] });
console.log('sent untrimmed, server got', await r.text());
r = await fetch(url, { headers: [['Last-Event-ID', asLatin1.trim()]] });
console.log('sent as web-fetch.js does, server got', await r.text());
try { new Request(url, { headers: [['X', 'é✓']] }); console.log('>255 accepted'); } catch (e) { console.log('>255 throws', e.name); }
console.log('typeof EventSource', typeof EventSource, 'WebSocket', typeof WebSocket, process.version);
srv.close();
