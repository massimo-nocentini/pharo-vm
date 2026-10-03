// serve.mjs - a static HTTP server for web/, the site of the Pharo VM for WebAssembly
//
//   node serve.mjs <dir> [port [host]]      (port 8080, host 127.0.0.1)
//
// `make wasm-serve' runs it, and the harnesses and the Playwright specs
// import it:
//
//   import { serve } from '.../serve.mjs';
//   const { url, close } = await serve(dir, { port: 0 });
//
// The pages need no cross-origin isolation (there is no SharedArrayBuffer),
// so there are no COOP and COEP headers.  .wasm goes as application/wasm, for
// WebAssembly.compileStreaming, and .gz as it is, since the worker inflates
// it.  Single ranges are honoured.  A URL with ?v= carries the build id and
// may be cached for good; the others are revalidated.

import { createReadStream, statSync } from 'node:fs';
import { createServer } from 'node:http';
import { extname, join, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

const TYPES = {
  '.changes': 'text/plain; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.gz': 'application/gzip',
  '.htm': 'text/html; charset=utf-8',
  '.html': 'text/html; charset=utf-8',
  '.ico': 'image/x-icon',
  '.image': 'application/octet-stream',
  '.js': 'text/javascript; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  '.mjs': 'text/javascript; charset=utf-8',
  '.png': 'image/png',
  '.sources': 'text/plain; charset=utf-8',
  '.st': 'text/plain; charset=utf-8',
  '.svg': 'image/svg+xml',
  '.txt': 'text/plain; charset=utf-8',
  '.wasm': 'application/wasm',
  '.webmanifest': 'application/manifest+json',
};

const stat = (path) => {
  try { return statSync(path); } catch (e) { return null; }
};

// The request handler for the files under root
export const handler = (root) => {
  root = resolve(root);
  return (request, response) => {
    const reply = (status, headers = {}, body = '') => {
      response.writeHead(status, { 'Content-Type': 'text/plain; charset=utf-8', ...headers });
      response.end(request.method == 'HEAD' ? undefined : body);
    };
    if (request.method != 'GET' && request.method != 'HEAD')
      return reply(405, { Allow: 'GET, HEAD' }, 'Method not allowed\n');

    const url = new URL(request.url, 'http://localhost');
    let path;
    try { path = decodeURIComponent(url.pathname); } catch (e) { path = null; }
    if (path == null || path.includes('\0')) return reply(400, {}, 'Bad request\n');
    let file = resolve(root, '.' + path);
    if (file != root && !file.startsWith(root + sep)) return reply(403, {}, 'Forbidden\n');
    let info = stat(file);
    if (info && info.isDirectory()) {
      if (!url.pathname.endsWith('/')) return reply(301, { Location: `${url.pathname}/${url.search}` });
      file = join(file, 'index.html');
      info = stat(file);
    }
    if (!info || !info.isFile()) return reply(404, {}, 'Not found\n');

    const headers = {
      'Content-Type': TYPES[extname(file).toLowerCase()] || 'application/octet-stream',
      'Accept-Ranges': 'bytes',
      'Cache-Control': url.searchParams.has('v') ? 'public, max-age=31536000, immutable' : 'no-cache',
      'Last-Modified': info.mtime.toUTCString(),
      'X-Content-Type-Options': 'nosniff',
    };
    const since = Date.parse(request.headers['if-modified-since'] || '');
    if (!request.headers.range && since >= Math.floor(info.mtimeMs / 1000) * 1000)
      return reply(304, headers);

    const size = info.size;
    let start = 0, end = size - 1, status = 200;
    const range = request.headers.range;
    if (range) {
      const match = /^bytes=(\d*)-(\d*)$/.exec(range.trim());
      if (match && (match[1] || match[2])) {
        if (match[1]) {
          start = Number(match[1]);
          if (match[2]) end = Math.min(Number(match[2]), size - 1);
        } else {
          start = Math.max(0, size - Number(match[2]));
        }
      }
      if (!match || !(match[1] || match[2]) || start > end || start >= size)
        return reply(416, { 'Content-Range': `bytes */${size}` });
      status = 206;
      headers['Content-Range'] = `bytes ${start}-${end}/${size}`;
    }
    headers['Content-Length'] = String(size == 0 ? 0 : end - start + 1);
    response.writeHead(status, headers);
    if (request.method == 'HEAD' || size == 0) return response.end();
    const stream = createReadStream(file, { start, end });
    stream.on('error', () => response.destroy());
    response.on('close', () => stream.destroy());
    stream.pipe(response);
  };
};

// Serves root; answers {server, url, close}.  Port 0 picks a free port.
export const serve = (root, { port = 8080, host = '127.0.0.1' } = {}) =>
  new Promise((resolvePromise, reject) => {
    const server = createServer(handler(root));
    server.on('error', reject);
    server.listen(port, host, () => {
      const address = server.address();
      const name = address.family == 'IPv6' ? `[${address.address}]` : address.address;
      resolvePromise({
        server,
        url: `http://${name}:${address.port}/`,
        close: () => new Promise((done) => server.close(done)),
      });
    });
  });

if (process.argv[1] && resolve(process.argv[1]) == fileURLToPath(import.meta.url)) {
  const [dir, port = '8080', host = '127.0.0.1'] = process.argv.slice(2);
  if (!dir || !/^\d+$/.test(port)) {
    process.stderr.write('usage: node serve.mjs <dir> [port [host]]\n');
    process.exit(2);
  }
  if (!stat(dir) || !stat(dir).isDirectory()) {
    process.stderr.write(`serve.mjs: ${dir} is not a directory (run make wasm first)\n`);
    process.exit(1);
  }
  const { url } = await serve(dir, { port: Number(port), host });
  const local = /^(127\.0\.0\.1|localhost|::1)$/.test(host) ? url.replace(/\/\/[^/]*:/, '//localhost:') : url;
  process.stdout.write(`Serving ${resolve(dir)} at ${local} (Ctrl-C stops)\n`);
}
