// fetch-http.mjs - local HTTP test server for WebFetch lanes and browser specs
//
//   import { startFetchHttp } from './lib/fetch-http.mjs';
//   const srv = await startFetchHttp({ host: '127.0.0.1', port: 0, cors: true });
//   // srv: { url, port, server, requests, get closed(), reset(), close() }
//
// CLI mode (for shell lanes):
//   node tests/wasm/lib/fetch-http.mjs [--no-cors] [--port PORT]
// prints the base URL on stdout and serves /_stats and /_reset control routes.

import http from 'node:http';
import zlib from 'node:zlib';
import { fileURLToPath } from 'node:url';

const PATTERN = Buffer.alloc(1 << 20);
for (let i = 0; i < PATTERN.length; i++) PATTERN[i] = i & 255;

const GZIP_PAYLOAD = zlib.gzipSync(Buffer.alloc(20000, 'abc'));

export async function startFetchHttp({ host = '127.0.0.1', port = 0, cors = true } = {}) {
  const requests = [];
  const timers = new Set();
  let closed = 0;
  let flakyCount = 0;

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
      const body = JSON.stringify({ closed, requests });
      res.writeHead(200, corsHeadersFor(cors, { 'Content-Type': 'application/json' }));
      res.end(body);
      return;
    }
    if (pathname === '/_reset' && req.method === 'POST') {
      requests.length = 0;
      closed = 0;
      flakyCount = 0;
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
        res.end(req.method === 'HEAD' ? undefined : 'hello');
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
        if (flakyCount++ < failCount) {
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

  return {
    url,
    port: addr.port,
    server,
    requests,
    get closed() { return closed; },
    reset() {
      requests.length = 0;
      closed = 0;
      flakyCount = 0;
    },
    close() {
      for (const t of timers) {
        clearTimeout(t);
        clearInterval(t);
      }
      timers.clear();
      return new Promise(resolve => {
        server.close(() => resolve());
        server.closeAllConnections();
      });
    },
  };
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const args = process.argv.slice(2);
  let cors = true;
  let port = 0;
  for (let i = 0; i < args.length; i++) {
    if (args[i] === '--no-cors') cors = false;
    else if (args[i] === '--port' && i + 1 < args.length) port = Number(args[++i]) || 0;
  }
  const srv = await startFetchHttp({ port, cors });
  console.log(srv.url);
  const stop = async () => { await srv.close(); process.exit(0); };
  process.on('SIGINT', stop);
  process.on('SIGTERM', stop);
}
