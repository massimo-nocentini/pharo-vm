// git-http.mjs - a git server over smart HTTP, and a CORS proxy in front of it, for the tests
//
//   import { hasGit, seedRepository, commitTo, fileOf, logOf, startGitHttp } from './lib/git-http.mjs';
//
// git.spec.mjs clones, fetches, pulls and pushes from a browser with these,
// through the smart-HTTP transport of libgit2 in the VM
// (src/emscripten/gitSupport.c):
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
//   fileOf(root, name, file, ref = 'master')
//                       the text of FILE in REF of ROOT/NAME.git
//   logOf(root, name, ref = 'master')
//                       the commits of REF in ROOT/NAME.git, newest first, as
//                       ['<subject> (<number of parents>)', ...]: what the
//                       host's git says a push left there
//   startGitHttp(root, {proxyHosts, redirect, push, auth, mover, onLog})
//                       starts two servers on 127.0.0.1 (three with mover),
//                       each on a port of its own, and answers {git, proxy,
//                       moved, log, close()}:
//
//     git     the URL of the git server, ending in /: `git http-backend'
//             serves the repositories of ROOT (GIT_PROJECT_ROOT), all of
//             them (GIT_HTTP_EXPORT_ALL), for fetches only (no push) unless
//             push is true.  It answers no CORS headers, so that a page of
//             another origin cannot read it: what a page gets from a git
//             server of its own, such as github.com.
//             With auth ({user, password, all}) the push asks for HTTP Basic
//             credentials, as github.com, GitLab and Codeberg do: a request
//             of git-receive-pack (GET .../info/refs?service=git-receive-pack
//             and POST .../git-receive-pack), or any request with all: true,
//             without the Authorization 'Basic base64(user:password)' gets
//             401, with WWW-Authenticate: Basic realm="git-http.mjs" and a
//             text/plain body that says why, as github.com says it ('No
//             anonymous write access.', or 'Invalid username or token.' for
//             other credentials); with them, `git http-backend' runs with
//             REMOTE_USER, the user, which lets it take the push;
//             With redirect: true it also does what GitLab and Codeberg do
//             with a remote named without its .git: for a repository
//             ROOT/<repo>.git, GET /<repo>/info/refs?... answers 301 to
//             /<repo>.git/info/refs?..., and POST /<repo>/git-upload-pack
//             answers 422, so that a client that does not take the
//             redirect for its next requests fails;
//     proxy   the URL of the CORS proxy, ending in /: a request for
//             <proxy><host>/<path> goes to http://<host>/<path>, as the
//             transport of the VM asks for it (the proxy, then the URL of
//             the remote without its scheme), and its response comes back
//             with the CORS headers that let any page read it, and the
//             preflights (OPTIONS) are answered.  As the proxy of
//             isomorphic-git (cors.isomorphic-git.org, the sites' default)
//             it lets a page send the headers Content-Type, Accept,
//             Git-Protocol and Authorization, and forwards those, drops the
//             WWW-Authenticate of a response (it passes on only Content-Type,
//             Location and its own CORS headers), and forwards only the
//             requests of git's smart HTTP: GET .../info/refs?service=
//             git-upload-pack or git-receive-pack, and POST .../git-upload-pack
//             or .../git-receive-pack with the content type of its service,
//             answering 403 with an empty body to any other.  It goes only to
//             the hosts of proxyHosts (host:port), by default the git server's
//             (and the mover's, with mover), and answers 403 with an empty body
//             to any other too: it is no open proxy.  It follows no redirect: a
//             Location of an http:// or https:// URL comes back made a path of
//             the proxy (/<host>/<path>), whatever its host, as isomorphic-git's
//             proxy makes it (index.js: location.replace(/^https?:\//, '')), so
//             that the browser follows it through the proxy, with the headers it
//             set (an Authorization too), and the proxy passes them on to that
//             host when it goes there;
//     moved   with mover, the URL of a third server, ending in /, which stands
//             for a git host that redirects to another one: on 127.0.0.1 too,
//             but named localhost (http://localhost:<port>/), which is
//             another host for a client than 127.0.0.1.  It answers
//             /adv/<path> with 301 to <git><path>, whatever the request (its
//             advertisement, and so the requests after it, go to the git
//             server); and /auth/<path> as a host that asks for credentials
//             and then redirects: a request of git-receive-pack without an
//             Authorization gets 401 (WWW-Authenticate: Basic, 'No anonymous
//             write access.'), one with any Authorization 302 to
//             <git><path>, and any other request 301 to <git><path>; 404
//             to anything else.  A client must not send the credentials it
//             has for localhost on to 127.0.0.1;
//     log     what each server got, in order: log.git, log.proxy and
//             log.mover are arrays of {method, url, origin, proxied,
//             authorization, preflight, status}, url the path and query that
//             it got, origin the Origin
//             header (a browser's request has one), proxied whether the proxy
//             forwarded it, authorization whether the request had an
//             Authorization header (never its value), preflight, for an
//             OPTIONS of the proxy, the headers that the browser asked to
//             send (Access-Control-Request-Headers, in lower case), status
//             what it answered (once it has);
//             onLog(server, entry), if given, is called with each entry
//             once it is answered, server 'git', 'proxy' or 'mover';
//     close() stops them.
//
// Run as a program, it serves ROOT until it is stopped, for a try by hand,
// and for tests/wasm/lanes/66-libgit2.sh:
//
//   node tests/wasm/lib/git-http.mjs ROOT [--redirect] [--push] [--auth USER:PASSWORD [--auth-all]]
//                                         [--mover] [--seed NAME]... [--log FILE]
//
// prints the URLs of the git server, of the mover (with --mover) and of the
// proxy, on lines 'git server <url> ...', 'mover <url>' and 'CORS proxy
// <url>', the proxy's last, once all listen.  --seed NAME first
// makes ROOT/NAME.git, a repository of Iceberg's layout (its code in src,
// in Tonel) with one commit, 'first'; --redirect, --push, --auth (with
// --auth-all for all: true) and --mover are the options above; --log FILE appends a
// line '<server> <method> <url> <status>' to FILE for each request
// answered, followed by ' authorization' when the request had an
// Authorization header.  Nothing here needs the network: the servers
// listen on 127.0.0.1.

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

export function fileOf(root, name, file, ref = 'master') {
  const r = spawnSync('git', ['show', ref + ':' + file], { cwd: path.join(root, name + '.git'), env: gitEnv(), encoding: 'utf8' });
  if (r.error) throw r.error;
  if (r.status !== 0) throw new Error(`git show ${ref}:${file}: exit status ${r.status}: ${(r.stderr || '').trim()}`);
  return r.stdout;
}

export function logOf(root, name, ref = 'master') {
  const out = git(['log', '--format=%s (%p)', ref], path.join(root, name + '.git'));
  return out ? out.split('\n').map(l => l.replace(/\(([^)]*)\)$/, (m, p) => '(' + (p.trim() ? p.trim().split(/\s+/).length : 0) + ')')) : [];
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
// (push: whether it takes a push, as REMOTE_USER, the user who gave the
// credentials, when there is one)
function backend(root, request, data, push = false, user = null) {
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
    if (user) env.REMOTE_USER = user;
    const child = spawn('git', ['-c', 'http.receivepack=' + (push ? 'true' : 'false'), 'http-backend'], { env });
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
  'access-control-allow-headers': 'content-type, accept, git-protocol, authorization',
  'access-control-max-age': '600',
};
// The requests of git's smart HTTP that the proxy forwards, as
// isomorphic-git's does: the path and query, and the content type of a POST
const SERVICES = /^(git-upload-pack|git-receive-pack)$/;
function gitRequest(request) {
  const [pathInfo, query = ''] = request.url.split('?');
  if (request.method === 'GET')
    return /\/info\/refs$/.test(pathInfo) && /^service=git-(upload|receive)-pack$/.test(query);
  const m = /\/(git-upload-pack|git-receive-pack)$/.exec(pathInfo);
  return request.method === 'POST' && !!m && SERVICES.test(m[1]) &&
    request.headers['content-type'] === 'application/x-' + m[1] + '-request';
}

// With auth: null when the request may go on, else the 401 that it gets
// (see above), and the user it authenticates otherwise
function challenge(auth, request) {
  const header = request.headers.authorization || '';
  const expected = 'Basic ' + Buffer.from(auth.user + ':' + auth.password).toString('base64');
  if (header === expected) return { user: auth.user };
  if (!auth.all && !/git-receive-pack/.test(request.url)) return { user: null };
  return { status: 401, headers: { 'www-authenticate': 'Basic realm="git-http.mjs"', 'content-type': 'text/plain' },
           body: Buffer.from(header ? 'Invalid username or token.\n' :
                             auth.all ? 'Authentication required.\n' : 'No anonymous write access.\n') };
}

// With redirect: what the git server answers itself for a remote named
// without its .git (see above), or null for git http-backend to answer
function renamed(root, request) {
  const [pathInfo, query] = request.url.split('?');
  const m = /^\/(.+)\/(info\/refs|git-upload-pack)$/.exec(pathInfo);
  if (!m || m[1].endsWith('.git') || m[1].split('/').includes('..')) return null;
  const repo = decodeURIComponent(m[1]);
  if (!fs.existsSync(path.join(root, repo + '.git'))) return null;
  if (m[2] === 'info/refs' && request.method === 'GET')
    return { status: 301, headers: { location: `/${m[1]}.git/info/refs${query === undefined ? '' : '?' + query}`,
                                     'content-type': 'text/plain' },
             body: Buffer.from('moved to ' + m[1] + '.git\n') };
  if (m[2] === 'git-upload-pack' && request.method === 'POST')
    return { status: 422, headers: { 'content-type': 'text/plain' },
             body: Buffer.from('git-http.mjs: ' + m[1] + ' is ' + m[1] + '.git, its redirect was not taken\n') };
  return null;
}

// The mover (see above): what it answers to request, a redirect to the git
// server gitUrl, a 401 or a 404
function moved(gitUrl, request) {
  const m = /^\/(adv|auth)(\/.*)$/.exec(request.url);
  if (!m) return { status: 404, headers: { 'content-type': 'text/plain' }, body: Buffer.from('git-http.mjs: no such path\n') };
  const to = { location: new URL(m[2].slice(1), gitUrl).href, 'content-type': 'text/plain' };
  if (m[1] === 'auth' && /git-receive-pack/.test(request.url)) {
    if (!request.headers.authorization)
      return { status: 401, headers: { 'www-authenticate': 'Basic realm="git-http.mjs mover"', 'content-type': 'text/plain' },
               body: Buffer.from('No anonymous write access.\n') };
    return { status: 302, headers: to, body: Buffer.from('found\n') };
  }
  return { status: 301, headers: to, body: Buffer.from('moved\n') };
}

export async function startGitHttp(root, { proxyHosts, redirect = false, push = false, auth = null, mover = false, onLog } = {}) {
  const log = { git: [], proxy: [], mover: [] };
  const answered = (server, entry, status) => {
    entry.status = status;
    if (onLog) onLog(server, entry);
  };
  const gitServer = http.createServer(async (request, response) => {
    const entry = { method: request.method, url: request.url, origin: request.headers.origin || null,
                    proxied: request.headers['x-git-http-proxy'] === '1', authorization: !!request.headers.authorization };
    try {
      const data = await body(request);
      log.git.push(entry);
      const asked = auth ? challenge(auth, request) : { user: null };
      const r = asked.status ? asked
        : (redirect && renamed(root, request)) || await backend(root, request, data, push || !!asked.user, asked.user);
      response.writeHead(r.status, r.headers);
      response.end(r.body);
      answered('git', entry, r.status);
    } catch (e) {
      response.writeHead(500, { 'content-type': 'text/plain' });
      response.end(String(e && e.message || e));
      answered('git', entry, 500);
    }
  });
  const gitUrl = await listen(gitServer);
  let moverServer = null, moverUrl = null;
  if (mover) {
    moverServer = http.createServer(async (request, response) => {
      const entry = { method: request.method, url: request.url, origin: request.headers.origin || null, proxied: false,
                      authorization: !!request.headers.authorization };
      await body(request);
      log.mover.push(entry);
      const r = moved(gitUrl, request);
      response.writeHead(r.status, r.headers);
      response.end(r.body);
      answered('mover', entry, r.status);
    });
    moverUrl = (await listen(moverServer)).replace('//127.0.0.1:', '//localhost:');
  }
  const allowed = new Set(proxyHosts || [new URL(gitUrl).host].concat(moverUrl ? [new URL(moverUrl).host] : []));
  const proxyServer = http.createServer(async (request, response) => {
    const entry = { method: request.method, url: request.url, origin: request.headers.origin || null, proxied: false,
                    authorization: !!request.headers.authorization };
    log.proxy.push(entry);
    if (request.method === 'OPTIONS') {
      entry.preflight = String(request.headers['access-control-request-headers'] || '').toLowerCase();
      response.writeHead(204, CORS);
      response.end();
      return answered('proxy', entry, 204);
    }
    // <proxy><host>/<path>: the URL of the remote without its scheme
    // (a refusal has an empty body, as isomorphic-git's proxy answers it)
    const m = /^\/([^/?#]+)(\/.*)?$/.exec(request.url);
    if (!m || !allowed.has(m[1]) || !gitRequest(request)) {
      response.writeHead(403, CORS);
      response.end();
      return answered('proxy', entry, 403);
    }
    try {
      const data = await body(request);
      const headers = { 'x-git-http-proxy': '1' };
      for (const k of ['content-type', 'accept', 'git-protocol', 'authorization'])
        if (request.headers[k]) headers[k] = request.headers[k];
      const r = await fetch('http://' + m[1] + (m[2] || '/'), {
        method: request.method, headers, body: request.method === 'POST' ? data : undefined, redirect: 'manual',
      });
      const answer = Buffer.from(await r.arrayBuffer());
      entry.proxied = true;
      const answers = { ...CORS, 'content-type': r.headers.get('content-type') || 'application/octet-stream',
                        'cache-control': 'no-cache' };
      // (a redirect stays in the proxy: http://<host>/<path> becomes
      // /<host>/<path>, whatever the host, as isomorphic-git's does)
      const location = r.headers.get('location');
      if (location) {
        const to = new URL(location, 'http://' + m[1] + (m[2] || '/'));
        if (/^https?:$/.test(to.protocol)) answers.location = '/' + to.host + to.pathname + to.search;
      }
      response.writeHead(r.status, answers);
      response.end(answer);
      answered('proxy', entry, r.status);
    } catch (e) {
      response.writeHead(502, { ...CORS, 'content-type': 'text/plain' });
      response.end('git-http.mjs: ' + String(e && e.message || e));
      answered('proxy', entry, 502);
    }
  });
  const proxyUrl = await listen(proxyServer);
  return {
    git: gitUrl, proxy: proxyUrl, moved: moverUrl, log,
    close: () => Promise.all([gitServer, proxyServer, moverServer].filter(s => s).map(s => new Promise(r => {
      s.close(() => r());
      s.closeAllConnections();
    }))),
  };
}

if (import.meta.url === pathToFileURL(process.argv[1] || '').href) {
  const args = process.argv.slice(2), seeds = [];
  let root = null, redirect = false, push = false, auth = null, all = false, mover = false, logFile = null;
  while (args.length) {
    const a = args.shift();
    if (a === '--redirect') redirect = true;
    else if (a === '--push') push = true;
    else if (a === '--auth' && args.length && args[0].includes(':')) {
      const pair = args.shift();
      auth = { user: pair.slice(0, pair.indexOf(':')), password: pair.slice(pair.indexOf(':') + 1) };
    } else if (a === '--auth-all') all = true;
    else if (a === '--mover') mover = true;
    else if (a === '--seed' && args.length) seeds.push(args.shift());
    else if (a === '--log' && args.length) logFile = path.resolve(args.shift());
    else if (!a.startsWith('--') && root === null) root = a;
    else {
      console.error('usage: node git-http.mjs ROOT [--redirect] [--push] [--auth USER:PASSWORD [--auth-all]] [--mover] [--seed NAME]... [--log FILE]');
      process.exit(2);
    }
  }
  root = path.resolve(root || '.');
  if (!hasGit()) {
    console.error('git-http.mjs: no git, or no git http-backend');
    process.exit(1);
  }
  // (the layout of a repository of Iceberg: its code in src, in Tonel)
  for (const name of seeds)
    seedRepository(root, name, {
      '.project': "{\n\t'srcDirectory' : 'src'\n}\n",
      'src/.properties': '{\n\t#format : #tonel\n}\n',
      'README.md': 'git-http.mjs\n',
    }, 'first');
  if (auth) auth.all = all;
  const onLog = logFile && ((server, e) => fs.appendFileSync(logFile,
    `${server} ${e.method} ${e.url} ${e.status}${e.authorization ? ' authorization' : ''}\n`));
  const s = await startGitHttp(root, { redirect, push, auth, mover, onLog });
  console.log(`git server ${s.git} (the repositories of ${root})` + (s.moved ? `\nmover ${s.moved}` : '') + `\nCORS proxy ${s.proxy}`);
}
