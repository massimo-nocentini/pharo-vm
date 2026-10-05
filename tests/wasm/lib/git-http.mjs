// git-http.mjs - a git server over smart HTTP, and a CORS proxy in front of it, for the tests
//
//   import { hasGit, seedRepository, commitTo, startGitHttp } from './lib/git-http.mjs';
//
// git.spec.mjs clones and fetches from a browser with these, through the
// smart-HTTP transport of libgit2 in the VM (src/emscripten/gitSupport.c):
//
//   hasGit()            whether the command git is there, and has
//                       `git http-backend'; answers its version, or null
//   seedRepository(root, name, files, message)
//                       makes ROOT/NAME.git, a bare repository, with one
//                       commit of FILES ({path: text}) on its branch master;
//                       answers the id of the commit
//   commitTo(root, name, files, message)
//                       adds a commit of FILES to the branch master of
//                       ROOT/NAME.git; answers its id
//   startGitHttp(root, {proxyHosts})
//                       starts two servers on 127.0.0.1, each on a port of
//                       its own, and answers {git, proxy, log, close()}:
//
//     git     the URL of the git server, ending in /: `git http-backend'
//             serves the repositories of ROOT (GIT_PROJECT_ROOT), all of
//             them (GIT_HTTP_EXPORT_ALL), for fetches only (no push).  It
//             answers no CORS headers, so that a page of another origin
//             cannot read it: what a page gets from a git server of its
//             own, such as github.com;
//     proxy   the URL of the CORS proxy, ending in /: a request for
//             <proxy><host>/<path> goes to http://<host>/<path>, as the
//             transport of the VM asks for it (the proxy, then the URL of
//             the remote without its scheme), and its response comes back
//             with the CORS headers that let any page read it, and the
//             preflights (OPTIONS) are answered.  It goes only to the hosts
//             of proxyHosts (host:port), by default the git server's, and
//             answers 403 to any other: it is no open proxy;
//     log     what each server got, in order: log.git and log.proxy are
//             arrays of {method, url, origin, proxied}, url the path and
//             query that it got, origin the Origin header (a browser's
//             request has one), proxied whether the proxy forwarded it;
//     close() stops both.
//
// Run as a program, it serves ROOT until it is stopped, for a try by hand:
//
//   node tests/wasm/lib/git-http.mjs ROOT
//
// prints the URLs of the git server and of the proxy.  Nothing here needs
// the network: both servers listen on 127.0.0.1.

import { spawn, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import http from 'node:http';
import os from 'node:os';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

// (git with no configuration of the user who runs the tests, and an author
// and a committer of its own)
const gitEnv = () => ({
  ...process.env,
  GIT_CONFIG_NOSYSTEM: '1',
  GIT_CONFIG_GLOBAL: os.devNull,
  GIT_AUTHOR_NAME: 'Git Spec', GIT_AUTHOR_EMAIL: 'git-spec@example.org',
  GIT_COMMITTER_NAME: 'Git Spec', GIT_COMMITTER_EMAIL: 'git-spec@example.org',
  GIT_TERMINAL_PROMPT: '0',
});
function git(args, cwd) {
  const r = spawnSync('git', args, { cwd, env: gitEnv(), encoding: 'utf8' });
  if (r.error) throw r.error;
  if (r.status !== 0) throw new Error(`git ${args.join(' ')}: exit status ${r.status}: ${(r.stderr || '').trim()}`);
  return r.stdout.trim();
}

export function hasGit() {
  const v = spawnSync('git', ['--version'], { encoding: 'utf8' });
  if (v.error || v.status !== 0) return null;
  // (http-backend answers its usage and an exit status of its own without a
  // request: what tells that it is missing is git's 'not a git command')
  const b = spawnSync('git', ['http-backend'], { encoding: 'utf8', env: { ...gitEnv(), REQUEST_METHOD: 'GET' } });
  if (b.error || /is not a git command/.test(b.stderr || '')) return null;
  return v.stdout.trim();
}

// FILES ({path: text}) written into the work tree DIR, and committed
function commitFiles(dir, files, message) {
  for (const [file, text] of Object.entries(files)) {
    fs.mkdirSync(path.dirname(path.join(dir, file)), { recursive: true });
    fs.writeFileSync(path.join(dir, file), text);
  }
  git(['add', '-A'], dir);
  git(['commit', '-q', '-m', message], dir);
  return git(['rev-parse', 'HEAD'], dir);
}

export function seedRepository(root, name, files, message = 'first') {
  const bare = path.join(root, name + '.git');
  const work = fs.mkdtempSync(path.join(root, '.seed-'));
  try {
    git(['init', '-q', '-b', 'master', work]);
    const id = commitFiles(work, files, message);
    git(['clone', '-q', '--bare', work, bare]);
    // (what `git clone' over HTTP asks for: the refs that the server
    // advertises, and nothing of the work tree it was made from)
    git(['remote', 'remove', 'origin'], bare);
    return id;
  } finally {
    fs.rmSync(work, { recursive: true, force: true });
  }
}

export function commitTo(root, name, files, message) {
  const bare = path.join(root, name + '.git');
  const work = fs.mkdtempSync(path.join(root, '.work-'));
  try {
    git(['clone', '-q', '-b', 'master', bare, work]);
    const id = commitFiles(work, files, message);
    git(['push', '-q', 'origin', 'master'], work);
    return id;
  } finally {
    fs.rmSync(work, { recursive: true, force: true });
  }
}

const body = request => new Promise((resolve, reject) => {
  const chunks = [];
  request.on('data', c => chunks.push(c));
  request.on('end', () => resolve(Buffer.concat(chunks)));
  request.on('error', reject);
});
const listen = server => new Promise((resolve, reject) => {
  server.once('error', reject);
  server.listen(0, '127.0.0.1', () => resolve('http://127.0.0.1:' + server.address().port + '/'));
});

// `git http-backend', a CGI program: the request goes to it in its
// environment and on its stdin, and it answers headers, a blank line and
// the body on its stdout
function backend(root, request, data) {
  return new Promise((resolve, reject) => {
    const [pathInfo, query = ''] = request.url.split('?');
    const env = {
      ...gitEnv(),
      GIT_PROJECT_ROOT: root, GIT_HTTP_EXPORT_ALL: '1',
      PATH_INFO: decodeURIComponent(pathInfo), QUERY_STRING: query, REQUEST_METHOD: request.method,
      CONTENT_TYPE: request.headers['content-type'] || '', CONTENT_LENGTH: String(data.length),
      REMOTE_ADDR: '127.0.0.1', SERVER_PROTOCOL: 'HTTP/1.1',
    };
    if (request.headers['git-protocol']) env.GIT_PROTOCOL = request.headers['git-protocol'];
    const child = spawn('git', ['-c', 'http.receivepack=false', 'http-backend'], { env });
    const out = [], err = [];
    child.stdout.on('data', d => out.push(d));
    child.stderr.on('data', d => err.push(d));
    child.on('error', reject);
    child.on('close', () => {
      const b = Buffer.concat(out), end = b.indexOf('\r\n\r\n');
      if (end < 0) return resolve({ status: 502, headers: { 'content-type': 'text/plain' },
                                    body: Buffer.from('git http-backend: ' + Buffer.concat(err).toString()) });
      let status = 200;
      const headers = {};
      for (const l of b.subarray(0, end).toString('latin1').split('\r\n')) {
        const at = l.indexOf(':');
        if (at < 0) continue;
        const k = l.slice(0, at).trim().toLowerCase(), v = l.slice(at + 1).trim();
        if (k === 'status') status = parseInt(v, 10) || 500;
        else headers[k] = v;
      }
      resolve({ status, headers, body: b.subarray(end + 4) });
    });
    child.stdin.on('error', () => { /* it may answer before it read all of it */ });
    child.stdin.end(data);
  });
}

const CORS = {
  'access-control-allow-origin': '*',
  'access-control-allow-methods': 'GET, POST, OPTIONS',
  'access-control-allow-headers': 'content-type, accept, git-protocol',
  'access-control-max-age': '600',
};

export async function startGitHttp(root, { proxyHosts } = {}) {
  const log = { git: [], proxy: [] };
  const gitServer = http.createServer(async (request, response) => {
    try {
      const data = await body(request);
      log.git.push({ method: request.method, url: request.url, origin: request.headers.origin || null,
                     proxied: request.headers['x-git-http-proxy'] === '1' });
      const r = await backend(root, request, data);
      response.writeHead(r.status, r.headers);
      response.end(r.body);
    } catch (e) {
      response.writeHead(500, { 'content-type': 'text/plain' });
      response.end(String(e && e.message || e));
    }
  });
  const gitUrl = await listen(gitServer);
  const allowed = new Set(proxyHosts || [new URL(gitUrl).host]);
  const proxyServer = http.createServer(async (request, response) => {
    const entry = { method: request.method, url: request.url, origin: request.headers.origin || null, proxied: false };
    log.proxy.push(entry);
    if (request.method === 'OPTIONS') {
      response.writeHead(204, CORS);
      return response.end();
    }
    // <proxy><host>/<path>: the URL of the remote without its scheme
    const m = /^\/([^/?#]+)(\/.*)?$/.exec(request.url);
    if (!m || !allowed.has(m[1])) {
      response.writeHead(403, { ...CORS, 'content-type': 'text/plain' });
      return response.end('git-http.mjs: this proxy goes only to ' + [...allowed].join(', '));
    }
    try {
      const data = await body(request);
      const headers = { 'x-git-http-proxy': '1' };
      for (const k of ['content-type', 'accept', 'git-protocol'])
        if (request.headers[k]) headers[k] = request.headers[k];
      const r = await fetch('http://' + m[1] + (m[2] || '/'), {
        method: request.method, headers, body: request.method === 'POST' ? data : undefined, redirect: 'manual',
      });
      const answer = Buffer.from(await r.arrayBuffer());
      entry.proxied = true;
      response.writeHead(r.status, { ...CORS, 'content-type': r.headers.get('content-type') || 'application/octet-stream',
                                     'cache-control': 'no-cache' });
      response.end(answer);
    } catch (e) {
      response.writeHead(502, { ...CORS, 'content-type': 'text/plain' });
      response.end('git-http.mjs: ' + String(e && e.message || e));
    }
  });
  const proxyUrl = await listen(proxyServer);
  return {
    git: gitUrl, proxy: proxyUrl, log,
    close: () => Promise.all([gitServer, proxyServer].map(s => new Promise(r => {
      s.close(() => r());
      s.closeAllConnections();
    }))),
  };
}

if (import.meta.url === pathToFileURL(process.argv[1] || '').href) {
  const root = path.resolve(process.argv[2] || '.');
  if (!hasGit()) {
    console.error('git-http.mjs: no git, or no git http-backend');
    process.exit(1);
  }
  const s = await startGitHttp(root);
  console.log(`git server ${s.git} (the repositories of ${root})\nCORS proxy ${s.proxy}`);
}
