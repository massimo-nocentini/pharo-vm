// fetch-http.mjs - local HTTP test server for WebFetch lanes and browser specs
//
//   import { startFetchHttp } from './lib/fetch-http.mjs';
//   const srv = await startFetchHttp({ host: '127.0.0.1', port: 0, cors: true, webSocket: true });
//   // srv: { url, port, hangPort, server, requests, ws, get closed(), get timerCount(), reset(), close() }
//
// CLI mode (for shell lanes):
//   node tests/wasm/lib/fetch-http.mjs [--no-cors] [--port PORT] [--hang-port]
// prints the base URL on stdout (with --hang-port, then a blank and the port of
// the second listener) and serves /_stats[?tag=TEXT] and /_reset control routes.
// GET /_stats?tag=TEXT filters requests and ws to entries whose url includes TEXT (closed stays the total).
// The requests log keeps at most the last 5000 entries (MAX_REQUESTS), mutating the same array object.
//
// With webSocket (the default, and the CLI's), the same server has the
// WebSocket endpoints of lib/ws-server.mjs under /ws/ (their connections in
// ws of /_stats), and a second listener on hangPort answers nothing (/ws/hang:
// a handshake that never ends, on a port of its own, since browsers make one
// handshake at a time to an address).  close() ends the upgraded sockets
// first, which server.close() would wait for.
//
// GET /sse?key=K&... is an event stream (text/event-stream; charset=utf-8, its
// head flushed at once).  The connections of each key are counted from 0
// (conn; /_reset and reset() start again), and a connection sends a BOM
// (bom=1), a line 'retry: R' (retry=R), a comment ': conn <conn>', then the
// events k = Number(Last-Event-ID) + 1, ..., ms apart:
//
//   id: <k>
//   event: tick
//   data: {"k":<k>,"conn":<conn>,"last":"<the Last-Event-ID, as UTF-8, or empty>"}
//   data: café ✓ 🙂              (utf8=1)
//
// and an empty line.  The parameters, with their defaults:
//
//   n=3        events per connection
//   ms=20      milliseconds between events
//   until=K    the series ends at the id K: no event after it, and 204 when
//              Number(Last-Event-ID) >= K, at the request
//   end=close  after the events: close (the body ends), destroy (ms later the
//              socket is destroyed: the body breaks), hang (open, silent), or
//              partial ('id: 99' and 'data: incomplete' without the empty
//              line, then the end)
//   ping=0     with end=hang, a comment ': ping' every ping ms
//   status=200 another status, with {"error":"status <code>"} as application/json
//   type=      another Content-Type
//   cc=no-store  the Cache-Control of every answer (max-age%3D60 makes it cacheable)
//   headDelay=0  milliseconds before the head
//   eol=lf     the line end: lf, crlf, cr, or mixed (LF, CR, CR LF, in turn:
//              so a CR is never followed by the LF of an empty line, which
//              would make one CR LF of the two line ends)
//   split=1    (implies utf8=1) each event in two writes, ms/2 apart, cut
//              inside the 2 bytes of the é, or between the CR and the LF of
//              the first data line in every other event with eol=crlf
//   size=S     first an event 'big', without id, whose data is S bytes of x

import http from 'node:http';
import zlib from 'node:zlib';
import { fileURLToPath } from 'node:url';
import { attachWebSocket, startHangServer } from './ws-server.mjs';

export const MAX_REQUESTS = 5000;

const PATTERN = Buffer.alloc(1 << 20);
for (let i = 0; i < PATTERN.length; i++) PATTERN[i] = i & 255;

const GZIP_PAYLOAD = zlib.gzipSync(Buffer.alloc(20000, 'abc'));

export async function startFetchHttp({ host = '127.0.0.1', port = 0, cors = true, webSocket = true } = {}) {
  const requests = [];
  const wsLog = [];
  const timers = new Set();
  let closed = 0;
  const flaky = new Map();     // how often /flaky was asked, per ?key=
  const sse = new Map();       // how often /sse was asked, per ?key=

  const corsHeadersFor = (enableCors, extra = {}) => (
    enableCors
      ? { 'Access-Control-Allow-Origin': '*', 'Access-Control-Expose-Headers': '*', ...extra }
      : { ...extra }
  );

  const server = http.createServer((req, res) => {
    req.on('error', () => {});
    res.on('error', () => {});

    const parsed = new URL(req.url, 'http://localhost');
    const pathname = parsed.pathname;

    // Control endpoints (not logged in requests)
    if (pathname === '/_stats' && req.method === 'GET') {
      const tag = parsed.searchParams.get('tag');
      const list = tag ? requests.filter(r => r.url.includes(tag)) : requests;
      const ws = tag ? wsLog.filter(r => r.url.includes(tag)) : wsLog;
      const body = JSON.stringify({ closed, requests: list, ws });
      res.writeHead(200, corsHeadersFor(cors, { 'Content-Type': 'application/json' }));
      res.end(body);
      return;
    }
    if (pathname === '/_reset' && req.method === 'POST') {
      requests.length = 0;
      wsLog.length = 0;
      closed = 0;
      flaky.clear();
      sse.clear();
      res.writeHead(204, corsHeadersFor(cors));
      res.end();
      return;
    }

    const enableCors = cors && pathname !== '/nocors' && parsed.searchParams.get('nocors') !== '1';
    const entry = {
      method: req.method,
      path: pathname,
      url: req.url,
      headers: { ...req.headers },
      origin: req.headers.origin || null,
      preflight: req.method === 'OPTIONS',
      status: null,
      closed: false,
    };
    requests.push(entry);
    if (requests.length > MAX_REQUESTS) {
      requests.splice(0, requests.length - MAX_REQUESTS);
    }

    let finished = false;
    res.on('finish', () => { finished = true; });
    res.on('close', () => {
      if (!finished) {
        entry.closed = true;
        closed++;
      }
    });

    const writeHead = (status, headers = {}) => {
      entry.status = status;
      res.writeHead(status, corsHeadersFor(enableCors, headers));
    };

    if (req.method === 'OPTIONS') {
      const reqHeaders = req.headers['access-control-request-headers'] || '*';
      entry.status = 204;
      res.writeHead(204, enableCors
        ? {
            'Access-Control-Allow-Origin': '*',
            'Access-Control-Allow-Methods': 'GET, HEAD, POST, PUT, PATCH, DELETE, OPTIONS',
            'Access-Control-Allow-Headers': reqHeaders,
            'Access-Control-Max-Age': '0',
          }
        : {});
      res.end();
      return;
    }

    switch (pathname) {
      case '/hello': {
        writeHead(200, { 'Content-Type': 'text/plain', 'Content-Length': 5, 'X-A': 'caf\xe9' });
        // a Buffer: node writes the head in the encoding of a string body, and X-A is Latin-1
        res.end(req.method === 'HEAD' ? undefined : Buffer.from('hello'));
        return;
      }
      case '/gzip': {
        writeHead(200, {
          'Content-Encoding': 'gzip',
          'Content-Type': 'text/plain',
          'Content-Length': GZIP_PAYLOAD.length,
        });
        res.end(req.method === 'HEAD' ? undefined : GZIP_PAYLOAD);
        return;
      }
      case '/204': {
        writeHead(204);
        res.end();
        return;
      }
      case '/redir': {
        const code = Number(parsed.searchParams.get('code')) || 302;
        const to = parsed.searchParams.get('to') || '/hello';
        writeHead(code, { Location: to });
        res.end();
        return;
      }
      case '/hang': {
        return;
      }
      case '/endless': {
        writeHead(200, { 'Content-Type': 'application/octet-stream' });
        let stopped = false;
        res.on('close', () => { stopped = true; });
        const pump = () => {
          while (!stopped && !res.destroyed) {
            if (!res.write(PATTERN.subarray(0, 65536))) {
              res.once('drain', pump);
              return;
            }
          }
        };
        pump();
        return;
      }
      case '/flaky': {
        const failCount = parsed.searchParams.has('fail') ? Number(parsed.searchParams.get('fail')) : 2;
        const key = parsed.searchParams.get('key') || '', asked = flaky.get(key) || 0;
        flaky.set(key, asked + 1);
        if (asked < failCount) {
          req.socket.destroy();
          return;
        }
        writeHead(200, { 'Content-Type': 'text/plain' });
        res.end('flaky ok');
        return;
      }
      case '/trunc': {
        writeHead(200, { 'Content-Type': 'application/octet-stream', 'Content-Length': 100000 });
        res.write(Buffer.alloc(5000));
        const t = setTimeout(() => {
          timers.delete(t);
          res.destroy();
        }, 100);
        timers.add(t);
        return;
      }
      case '/dribble': {
        const ticks = Number(parsed.searchParams.get('ticks')) || 5;
        const ms = Number(parsed.searchParams.get('ms')) || 50;
        writeHead(200, { 'Content-Type': 'text/plain' });
        let i = 0;
        const t = setInterval(() => {
          if (res.destroyed) {
            clearInterval(t);
            timers.delete(t);
            return;
          }
          res.write('tick' + i);
          if (++i >= ticks) {
            clearInterval(t);
            timers.delete(t);
            res.end();
          }
        }, ms);
        timers.add(t);
        res.on('close', () => {
          clearInterval(t);
          timers.delete(t);
        });
        return;
      }
      case '/n': {
        let remaining = Math.max(0, Number(parsed.searchParams.get('n')) || 0);
        writeHead(200, { 'Content-Type': 'application/octet-stream' });
        let stopped = false;
        res.on('close', () => { stopped = true; });
        const pump = () => {
          while (remaining > 0 && !stopped && !res.destroyed) {
            const k = Math.min(remaining, PATTERN.length);
            remaining -= k;
            if (!res.write(PATTERN.subarray(0, k))) {
              res.once('drain', pump);
              return;
            }
          }
          if (!stopped && !res.destroyed) res.end();
        };
        pump();
        return;
      }
      case '/echo': {
        const chunks = [];
        req.on('data', d => chunks.push(d));
        req.on('end', () => {
          const buf = Buffer.concat(chunks);
          const body = [
            req.method,
            req.url,
            req.headers['content-type'] || '',
            req.headers.authorization || '',
            req.headers['user-agent'] || '',
            String(buf.length),
            buf.toString('utf8', 0, 60),
          ].join('|');
          writeHead(200, { 'Content-Type': 'text/plain' });
          res.end(body);
        });
        return;
      }
      case '/nocors': {
        writeHead(200, { 'Content-Type': 'text/plain' });
        res.end('nocors');
        return;
      }
      case '/sse': {
        const q = parsed.searchParams;
        const number = (name, value) => q.has(name) ? Number(q.get(name)) : value;
        const key = q.get('key') || '', conn = sse.get(key) || 0;
        sse.set(key, conn + 1);
        const header = req.headers['last-event-id'];
        // node takes the bytes of a header as Latin-1
        const last = header === undefined ? '' : Buffer.from(header, 'latin1').toString('utf8');
        const from = Number(header) || 0, until = number('until', Infinity);
        const n = number('n', 3), ms = number('ms', 20), ping = number('ping', 0), status = number('status', 200);
        const end = q.get('end') || 'close', eol = q.get('eol') || 'lf', cc = q.get('cc') || 'no-store';
        const split = q.get('split') === '1', utf8 = split || q.get('utf8') === '1', size = number('size', 0);
        // the timers of this answer, cleared when it closes
        const own = new Set();
        const forget = t => { own.delete(t); timers.delete(t); };
        const later = (f, wait) => {
          const t = setTimeout(() => { forget(t); f(); }, wait);
          own.add(t);
          timers.add(t);
        };
        const every = (f, wait) => {
          const t = setInterval(f, wait);
          own.add(t);
          timers.add(t);
        };
        res.on('close', () => {
          for (const t of [...own]) {
            clearTimeout(t);
            forget(t);
          }
        });
        let line = 0;
        const ending = () => eol === 'mixed' ? ['\n', '\r', '\r\n'][line++ % 3] : ({ crlf: '\r\n', cr: '\r' })[eol] || '\n';
        const lines = list => list.map(each => each + ending()).join('');
        // the event of the id k, in one or two Buffers
        const event = (k, j) => {
          const parts = ['id: ' + k + ending(), 'event: tick' + ending()];
          const json = 'data: ' + JSON.stringify({ k, conn, last }), jsonEnd = ending();
          parts.push(json + jsonEnd);
          if (utf8) parts.push('data: caf\u00e9 \u2713 \u{1f642}' + ending());
          parts.push(ending());
          const bytes = Buffer.from(parts.join(''), 'utf8');
          if (!split) return [bytes];
          const before = Buffer.byteLength(parts[0] + parts[1]);
          const at = jsonEnd === '\r\n' && j % 2 === 0
            ? before + Buffer.byteLength(json) + 1
            : before + Buffer.byteLength(parts[2]) + Buffer.byteLength('data: caf') + 1;
          return [bytes.subarray(0, at), bytes.subarray(at)];
        };
        const count = Math.max(0, Math.min(n, until - from));
        const finish = () => {
          if (end === 'destroy') later(() => req.socket.destroy(), ms);
          else if (end === 'hang') { if (ping > 0) every(() => res.write(lines([': ping'])), ping); }
          else if (end === 'partial') res.end(lines(['id: 99', 'data: incomplete']));
          else res.end();
        };
        const send = j => {
          if (j >= count) return finish();
          const pieces = event(from + 1 + j, j);
          res.write(pieces[0]);
          const rest = () => {
            if (pieces[1]) res.write(pieces[1]);
            if (j + 1 < count) later(() => send(j + 1), pieces[1] ? ms - ms / 2 : ms);
            else finish();
          };
          if (pieces[1]) later(rest, ms / 2);
          else rest();
        };
        const answer = () => {
          if (from >= until) {
            writeHead(204, { 'Cache-Control': cc });
            res.end();
          } else if (status !== 200) {
            writeHead(status, { 'Content-Type': 'application/json', 'Cache-Control': cc });
            res.end(JSON.stringify({ error: 'status ' + status }));
          } else {
            writeHead(200, { 'Content-Type': q.get('type') || 'text/event-stream; charset=utf-8', 'Cache-Control': cc });
            res.flushHeaders();
            res.write((q.get('bom') === '1' ? '\ufeff' : '') + (q.has('retry') ? lines(['retry: ' + q.get('retry')]) : '')
              + lines([': conn ' + conn]) + (size > 0 ? lines(['event: big', 'data: ' + 'x'.repeat(size), '']) : ''));
            send(0);
          }
        };
        if (number('headDelay', 0) > 0) later(answer, number('headDelay', 0));
        else answer();
        return;
      }
      case '/status': {
        const code = Number(parsed.searchParams.get('code')) || 200;
        writeHead(code, { 'Content-Type': 'text/plain' });
        res.end(String(code));
        return;
      }
      default: {
        writeHead(404, { 'Content-Type': 'text/plain' });
        res.end('none');
      }
    }
  });

  server.on('connection', socket => {
    socket.on('error', () => {});
  });

  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(port, host, () => {
      server.removeListener('error', reject);
      resolve();
    });
  });

  const addr = server.address();
  const url = `http://${host}:${addr.port}`;
  const ws = webSocket ? attachWebSocket(server, { log: wsLog }) : null;
  const hang = webSocket ? await startHangServer({ host, log: wsLog }) : null;

  return {
    url,
    port: addr.port,
    hangPort: hang ? hang.port : null,
    server,
    requests,
    ws: wsLog,
    get closed() { return closed; },
    get timerCount() { return timers.size; },
    reset() {
      requests.length = 0;
      wsLog.length = 0;
      closed = 0;
      flaky.clear();
      sse.clear();
    },
    close() {
      for (const t of timers) {
        clearTimeout(t);
        clearInterval(t);
      }
      timers.clear();
      if (ws) ws.close();
      return Promise.all([
        new Promise(resolve => {
          server.close(() => resolve());
          server.closeAllConnections();
        }),
        hang ? hang.close() : null,
      ]).then(() => {});
    },
  };
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const args = process.argv.slice(2);
  let cors = true;
  let port = 0;
  let hangPort = false;
  for (let i = 0; i < args.length; i++) {
    if (args[i] === '--no-cors') cors = false;
    else if (args[i] === '--hang-port') hangPort = true;
    else if (args[i] === '--port' && i + 1 < args.length) port = Number(args[++i]) || 0;
  }
  const srv = await startFetchHttp({ port, cors });
  console.log(hangPort ? srv.url + ' ' + srv.hangPort : srv.url);
  const stop = async () => { await srv.close(); process.exit(0); };
  process.on('SIGINT', stop);
  process.on('SIGTERM', stop);
}
