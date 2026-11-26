// websocket.spec.mjs - the WebSockets of the image in the pages, in a browser
//
//   PLAYWRIGHT_MODULE=... [WEB_FETCH_DIR=...] node tests/wasm/websocket.spec.mjs WEB_DIR
//
// The Console, the Notebook and the World of a build, with the primitives of
// the WebSocketPlugin (tests/wasm/st/websocket-spec.st), against the
// WebSocket endpoints of tests/wasm/lib/fetch-http.mjs (lib/ws-server.mjs),
// reached as localhost: another origin, and another hostname than the
// site's 127.0.0.1, so that the refusal of the hosts of the site's domain
// (an IP address: only itself) does not apply to them.
//
// Console: echo of text, binary and 1 MiB; the subprotocol chosen; the
// Origin of the site and a cookie of localhost (context.addCookies) go with
// the handshake; the site's own hostname is refused, and nothing reaches the
// server; a leading BOM, as the browser delivers it; three handshakes to
// /ws/echo while one to the second port of the server never ends, and
// whether a second one to that port reaches the server; Stop of a
// wait for a message, after which the socket goes on; 1500 messages at
// once, which the Console prints one by one, more than the output that the
// worker lets wait (1 MiB): the socket ends closed, or failed with overflow,
// and the worker goes on; no violation of the CSP.  Notebook: a cell reads a push, a process of a cell
// waits for a message while other cells run.  World: a reader process
// updates a morph through UIManager default defer:, and Stop of a wait in the
// UI process opens a debugger, after which the world draws.  The setting of
// the network, with a WebSocket of the image and a socket of the SocketPlugin
// (Emscripten's, to a websockify-style server of the spec) open: off, the
// first fails with off and the server gets 1000, the second closes, then
// both are refused and nothing reaches either server; on again, both work;
// another tab follows; the Notebook too, and the world.
//
// It measures, and prints as '# ...' lines (docs/WebSocket.md has them), and
// does not check against a constant: how large bufferedAmount grows against
// /ws/slow-read before a send is refused; when the browser ends TCP after a
// close that /ws/deaf never answers (the close timer of web-socket.js ends
// the socket first); the BOM.
//
// With WEB_FETCH_DIR, a checkout of the package (web-fetch.st): the suite of
// WebFetch-WebSocket-Tests runs in the Console, with the live tests against
// the server (only WebFetchWebSocketNativeTest and
// WebFetchWebSocketEchoServerTest skip).
//
// Only Chromium is run: of the browsers that BROWSERS names, the others
// are skipped, with a line that says so, and with no chromium among them
// nothing runs.  The cookie check relies on Chromium's rules (Firefox
// partitions third-party cookies), and the numbers of the measures are
// Chromium's.

import crypto from 'node:crypto';
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { run } from './lib/pw.mjs';
import { startFetchHttp } from './lib/fetch-http.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const { handler } = await import(new URL('../../packaging/emscripten/tools/serve.mjs', import.meta.url));
const packageDir = process.env.WEB_FETCH_DIR ? path.resolve(process.env.WEB_FETCH_DIR) : null;
if (packageDir && !fs.existsSync(path.join(packageDir, 'src', 'BaselineOfWebFetch')))
  throw new Error(`no ${packageDir}/src/BaselineOfWebFetch (WEB_FETCH_DIR)`);

// The files added to those of every VM: {path, data}; websocket.lane is
// what tests/run-websocket.st of the package reads
function laneFiles(lane) {
  const files = [
    { path: '/pharo/lane/websocket-spec.st', data: fs.readFileSync(path.join(here, 'st', 'websocket-spec.st'), 'utf8') },
    { path: '/pharo/lane/git-world.st', data: fs.readFileSync(path.join(here, 'st', 'git-world.st'), 'utf8') },
    { path: '/pharo/websocket.lane', data: lane + '\n' },
  ];
  const walk = (dir, to) => {
    for (const name of fs.readdirSync(dir)) {
      const file = path.join(dir, name);
      if (fs.statSync(file).isDirectory()) walk(file, to + '/' + name);
      else files.push({ path: to + '/' + name, data: fs.readFileSync(file, 'utf8') });
    }
  };
  if (packageDir) {
    walk(path.join(packageDir, 'src'), '/pharo/lane/web-fetch.st/src');
    walk(path.join(packageDir, 'tests'), '/pharo/lane/web-fetch.st/tests');
    files.push({ path: '/pharo/lane/web-fetch.st/.project', data: fs.readFileSync(path.join(packageDir, '.project'), 'utf8') });
  }
  return files;
}
// Appended to vm-driver.js: the files, and the evaluator of the world
const patch = files => `
;(function () {
  // websocket.spec.mjs
  var D = self.PharoVMDriver, start = D.start, vmArgs = D.vmArgs, files = ${JSON.stringify(files)};
  D.vmArgs = function (mode) {
    var args = vmArgs.apply(this, arguments);
    return mode === 'world' ? args.concat(['st', '/pharo/lane/git-world.st']) : args;
  };
  D.start = function (create, o) {
    o.files = (o.files || []).concat(files);
    return start.call(this, create, o);
  };
})();
`;

// A server of the WebSockets of Emscripten's sockets (websockify-style: one
// subprotocol, binary, and the bytes of TCP in frames), which counts what
// comes
async function startSockets() {
  const log = { sockets: 0, closed: 0, frames: 0 };
  const open = new Set();
  const server = http.createServer((q, r) => { r.writeHead(404); r.end(); });
  server.on('connection', s => { open.add(s); s.on('close', () => open.delete(s)); s.on('error', () => {}); });
  server.on('upgrade', (q, s) => {
    log.sockets++;
    const key = crypto.createHash('sha1').update(q.headers['sec-websocket-key'] + '258EAFA5-E914-47DA-95CA-C5AB0DC85B11').digest('base64');
    s.write('HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n' +
            `Sec-WebSocket-Accept: ${key}\r\nSec-WebSocket-Protocol: binary\r\n\r\n`);
    s.on('data', d => {
      if ((d[0] & 15) !== 8) { log.frames++; return; }
      s.write(Buffer.from([0x88, 0]));
      s.end();
    });
    s.on('end', () => s.end());
    s.on('close', () => { log.closed++; });
  });
  await new Promise(f => server.listen(0, '127.0.0.1', f));
  return { log, port: server.address().port,
           close: () => { for (const s of open) s.destroy(); server.close(); } };
}

const named = (process.env.BROWSERS || 'chromium').split(',').map(s => s.trim()).filter(Boolean);
const others = named.filter(name => name !== 'chromium');
if (others.length) console.log(`# websocket.spec.mjs runs in Chromium only: ${others.join(', ')} skipped`);
if (!named.includes('chromium')) {
  console.log('# websocket.spec.mjs: no chromium in BROWSERS, nothing run');
  process.exit(0);
}
process.env.BROWSERS = 'chromium';

await run(async t => {
  const { check, assert, manifest } = t;
  await t.page.close();
  const web = await startFetchHttp(), other = await startSockets();
  // the endpoints, as localhost; B for the fetches of /_stats by the package
  const B = `http://localhost:${web.port}`, W = `ws://localhost:${web.port}`, hang = `ws://localhost:${web.hangPort}/ws/hang`;

  // ---- the site, with the files of the lane
  const files = handler(t.webDir), driver = fs.readFileSync(path.join(t.webDir, 'vm-driver.js'), 'utf8') +
    patch(laneFiles(B + ' ' + web.hangPort));
  const site = http.createServer((q, r) => {
    const p = new URL(q.url, 'http://x').pathname;
    const reply = (type, body) => { r.writeHead(200, { 'Content-Type': type, 'Cache-Control': 'no-cache' }); r.end(body); };
    if (p === '/vm-driver.js') return reply('text/javascript; charset=utf-8', driver);
    if (p === '/manifest.json')
      return reply('application/json', JSON.stringify(Object.assign({}, manifest, { gitProxyDefault: 'http://127.0.0.1:9/' })));
    files(q, r);
  });
  await new Promise(f => site.listen(0, '127.0.0.1', f));
  const base = `http://127.0.0.1:${site.address().port}/`, origin = base.slice(0, -1);

  // ---- the pages, in a context without a route
  const context = async () => {
    const c = await t.browser.newContext({ viewport: { width: 1280, height: 800 } });
    await c.addInitScript(() => {
      window.__csp = [];
      document.addEventListener('securitypolicyviolation', e => window.__csp.push(e.violatedDirective + ' ' + e.blockedURI));
    });
    return c;
  };
  const ctx = await context();
  // a cookie of the host of the server, which the handshakes carry: the
  // site is another one, so only a SameSite=None cookie goes, which must be
  // Secure (Chromium takes localhost for a secure host, and sends it to ws:
  // too)
  await ctx.addCookies([{ name: 'wscookie', value: '1', domain: 'localhost', path: '/', sameSite: 'None', secure: true }]);
  const newPage = async (c = ctx) => {
    const p = await c.newPage();
    t.watch(p);
    p.on('dialog', d => d.accept().catch(() => {}));
    return p;
  };
  // run f, whose errors of the console that match re are asked for
  const expecting = async (re, f) => {
    const at = t.errors.length;
    try { return await f(); } finally {
      await new Promise(f => setTimeout(f, 300));
      for (let i = t.errors.length - 1; i >= at; i--) if (re.test(t.errors[i])) t.errors.splice(i, 1);
    }
  };
  // (a socket that fails, or is closed before it opens, is a console error
  // of the worker in Chromium)
  const SOCKET_ERRORS = /WebSocket|ws:\/\//;
  const waitFor = async (what, pred, ms = 10000) => {
    const t0 = Date.now();
    for (;;) {
      const v = await pred();
      if (v) return v;
      if (Date.now() - t0 > ms) throw new Error('timed out waiting for ' + what);
      await new Promise(f => setTimeout(f, 50));
    }
  };
  // the connections that the server logged whose URL has tag
  const logged = tag => web.ws.filter(e => e.url.includes(tag));
  const ended = (tag, ms = 10000) => waitFor('the end of ' + tag, () => {
    const l = logged(tag);
    return l.length && l.every(e => e.ended) && l[0];
  }, ms);
  const set = async (p, on, menu) => {
    if (menu) await menu(); else await p.click('#settings');
    await p.setChecked('#network-enabled', on);
    await p.click('#settings-save');
    await p.waitForFunction(() => !document.getElementById('settings-dialog').open);
  };

  // ---- the Console of a page p: ev evaluates a line, and answers what is
  // between the double brackets of its value
  const term = p => p.$eval('#term', e => e.textContent);
  const waiting = p => p.waitForFunction(() => document.getElementById('status').dataset.state === 'waiting' &&
    /(^|\n)st> $/.test(document.getElementById('term').textContent), null, { timeout: 120000 });
  const type = async (p, source) => {
    const at = (await term(p)).length;
    await p.fill('#line', source);
    await p.press('#line', 'Enter');
    return at;
  };
  const ev = async (p, source, timeout = 60000) => {
    const at = await type(p, source);
    const h = await p.waitForFunction(at => {
      // (the scrollback is trimmed past TERM_CHARS of page.js: then at is
      // past its end, and the whole of it is looked at)
      const all = document.getElementById('term').textContent, text = at <= all.length ? all.slice(at) : all;
      if (document.getElementById('status').dataset.state !== 'waiting' || !/\nst> $/.test(text)) return false;
      const m = /\[\[([^]*)\]\]'\n/.exec(text.slice(Math.max(0, text.lastIndexOf('[['))));
      return m ? m[1] : '!' + text.slice(-2000);
    }, at, { timeout });
    return (await h.jsonValue()).replace(/''/g, "'");
  };
  const evalFile = file => `Smalltalk compiler evaluate: '${file}' asFileReference contents`;
  const prepare = async p => {
    const said = await ev(p, evalFile('/pharo/lane/websocket-spec.st'));
    if (said !== 'ready') throw new Error('websocket-spec.st: ' + said);
  };
  const is = (got, want, what = '') => assert(got === want, `${what} got ${JSON.stringify(got)}, want ${JSON.stringify(want)}`);

  // the network on and off, in an evaluator say: a WebSocket of the image
  // and a socket of the SocketPlugin
  const checkOn = async (say, tag) => {
    is(await say('WSP available'), 'availability 1');
    is(await say(`WSP connect: #${tag} url: '${W}/ws/echo?t=${tag}'`), `2|||${W}/ws/echo?t=${tag}`);
    is(await say(`WSP send: #${tag} text: 'on'`), 'sent true');
    is(await say(`WSP nextOf: #${tag}`), 'on');
    is(await say(`WSP socket: ${other.port}`), 'socket true');
  };
  const checkGoesOff = async (say, off, tag) => {
    await expecting(SOCKET_ERRORS, async () => {
      await off();
      is(await say(`WSP status: #${tag}`), `5|off|network access is off|||${W}/ws/echo?t=${tag}`);
      const e = await ended('t=' + tag);
      assert(e.code === 1000 && e.reason === 'network access is off', 'the server got ' + JSON.stringify(e));
      await waitFor('the end of the sockets', () => other.log.closed === other.log.sockets);
    });
    const frames = other.log.frames, sent = await say('WSP socketSend');
    assert(other.log.frames === frames, 'nothing on the socket: ' + sent);
    is(await say(`WSP free: #${tag}`), 'released true');
  };
  const checkOff = async (say, tag) => {
    const n = web.ws.length, before = JSON.stringify(other.log);
    is(await say('WSP available'), 'availability 2');
    is(await say(`WSP open: #${tag} url: '${W}/ws/echo?t=${tag}'`), 'unsupported operation');
    const socket = await say(`WSP socket: ${other.port}`);
    assert(/^socket [A-Z]\w+$/.test(socket), socket);
    assert(web.ws.length === n && JSON.stringify(other.log) === before, 'nothing was sent: ' + JSON.stringify(other.log));
  };

  // ---- the Console
  const page = await newPage();
  await page.goto(base + 'index.html');
  await waiting(page);
  await prepare(page);
  const say = (source, ms) => ev(page, source, ms);

  await check('Console: echo of text, binary and 1 MiB; the subprotocol; the Origin of the site and the cookie of localhost', async () => {
    is(await say('WSP available'), 'availability 1');
    is(await say(`WSP open: #e url: '${W}/ws/echo?t=c1' protocols: #( 'x' 'chat' ) maxQueued: 16777216 timeout: 10000`), 'opened');
    is(await say('WSP status: #e'), `2|chat||${W}/ws/echo?t=c1`);
    is(await say('WSP echo: #e'), 'true|#[0 1 2 255]|true');
    const [e] = logged('t=c1');
    assert(e.origin === origin && e.cookie === true, JSON.stringify(e));
    assert(e.offered.join() === 'x,chat' && /permessage-deflate/.test(e.extensions), 'offered ' + JSON.stringify(e));
    is(await say('WSP free: #e'), 'released true');
    is((await ended('t=c1')).code, 1000, 'the code of release');
  });

  await check('Console: the hostname of the site is refused, with any port, and nothing reaches the server', async () => {
    const n = web.ws.length;
    for (const u of [`ws://127.0.0.1:${web.port}/ws/echo`, `ws://127.0.0.1:${site.address().port}/`]) {
      is(await say(`WSP connect: #own url: '${u}'`), '5|request|a host of the domain of this site is refused: its cookies would be sent|||');
      is(await say('WSP free: #own'), 'released true');
    }
    is(web.ws.length, n, 'connections');
  });

  await check('Console: a leading BOM, as Chromium delivers it', async () => {
    is(await say(`WSP connect: #bom url: '${W}/ws/push?n=1&bom=1&close=none'`), `2|||${W}/ws/push?n=1&bom=1&close=none`);
    const said = await say('WSP firstBytesOf: #bom');
    // U+FEFF then '1': kept, or stripped
    assert(said === '#[239 187 191 49]' || said === '#[49]', said);
    console.log(`# a text message that begins with U+FEFF arrives as ${said === '#[49]' ? 'stripped' : 'kept'}: ${said}`);
    is(await say('WSP free: #bom'), 'released true');
  });

  await check('Console: three handshakes to /ws/echo while one to the second port never ends, and a second one to that port', async () => {
    await expecting(SOCKET_ERRORS, async () => {
      is(await say(`WSP open: #hang url: '${hang}' protocols: #() maxQueued: 1000 timeout: 0`), 'opened');
      await waitFor('the hanging handshake', () => logged('/ws/hang').length);
      const said = await say(`| t | t := Time millisecondClockValue. #(#h1 #h2 #h3) do: [ :n | WSP open: n url: '${W}/ws/echo?t=' , n ]. ` +
        "#(#h1 #h2 #h3) do: [ :n | WSP waitFor: n until: [ :st :c | st >= 2 ] ]. " +
        "WSP say: (Time millisecondClockValue - t) printString , ' ' , (#(#h1 #h2 #h3 #hang) collect: [ :n | WSP st: n ]) printString");
      const m = /^(\d+) #\(2 2 2 1\)$/.exec(said);
      assert(m && Number(m[1]) < 1000, said);
      console.log(`# three handshakes behind a pending one to another port: ${m[1]} ms`);
      // measured, not checked: does the browser hold back a handshake to the port of a pending one?
      is(await say(`WSP open: #hang2 url: '${hang}?t=2' protocols: #() maxQueued: 1000 timeout: 0`), 'opened');
      await new Promise(r => setTimeout(r, 1500));
      const reached = logged('/ws/hang').length;
      console.log(`# a second handshake to the port of a pending one, after 1.5 s: ${reached === 2 ? 'it reached the server' : 'held back by the browser'} (${reached} of 2 at the server)`);
      is(await say('WSP freeAll'), 'released all');
    });
  });

  const stopIn = async (p, at) => {
    await p.waitForTimeout(300);
    await p.click('#stop');
    await p.waitForFunction(at => /Interrupted\.\nst> $/.test(document.getElementById('term').textContent.slice(at)), at, { timeout: 5000 });
  };
  await check('Console: Stop of a wait for a message, and the socket goes on', async () => {
    is(await say(`WSP connect: #s url: '${W}/ws/echo?t=stop'`), `2|||${W}/ws/echo?t=stop`);
    await stopIn(page, await type(page, 'WSP waitNextOf: #s'));
    is(await say('WSP say: (WSP st: #s) printString'), '2');
    is(await say("WSP send: #s text: 'after the Stop'"), 'sent true');
    is(await say('WSP nextOf: #s'), 'after the Stop');
    is(await say('WSP free: #s'), 'released true');
  });

  await check('Console: 1500 messages at once, which the Console prints one by one (1.5 MB of output)', async () => {
    const t0 = Date.now();
    is(await say(`WSP open: #f url: '${W}/ws/push?n=1500&size=1024&t=flood' protocols: #() maxQueued: 16777216 timeout: 10000`), 'opened');
    const said = await say('WSP flood: #f', 180000);
    const m = /^(\d+)\|(4|5)\|?(\w*)$/.exec(said);
    assert(m && (m[2] === '4' || m[3] === 'overflow'), said.slice(0, 300));
    console.log(`# 1500 messages of 1 KiB at once, printed one by one: ${m[1]} read, state ${m[2]} ${m[3]}, ${Date.now() - t0} ms`);
    is(await say('WSP free: #f'), 'released true');
    is(await say('WSP available'), 'availability 1', 'the worker goes on');
  });

  // the close of /ws/deaf, and when the server saw TCP end, which the last
  // check says
  let deafClosed = 0, deafEnded = 0;
  const deafWatch = setInterval(() => {
    if (deafClosed && !deafEnded && logged('t=deafm').some(e => e.ended)) deafEnded = Date.now();
  }, 100);
  await check('measures: bufferedAmount against /ws/slow-read, and a close that /ws/deaf never answers', async () => {
    is(await say(`WSP connect: #slow url: '${W}/ws/slow-read?resume=5000&t=slow'`), `2|||${W}/ws/slow-read?resume=5000&t=slow`);
    const said = await say('WSP fill: #slow', 120000);
    const m = /^(\d+)\|(\d+)\|#\(#'resource limit exceeded'\)\|true$/.exec(said);
    assert(m, said);
    console.log(`# /ws/slow-read: ${m[1]} sends of 1 MiB before one was refused, bufferedAmount up to ${m[2]}; the write semaphore signalled once it read`);
    is(await say('WSP free: #slow'), 'released true');
    is(await say(`WSP connect: #deaf url: '${W}/ws/deaf?t=deafm'`), `2|||${W}/ws/deaf?t=deafm`);
    deafClosed = Date.now();
    const closed = await say('WSP closeTimed: #deaf');
    const d = /^(\d+)\|4\|\|\|[^|]*\|1006\|closing handshake timed out\|0$/.exec(closed);
    assert(d && Number(d[1]) < 6000, closed);
    console.log(`# /ws/deaf: the close timer ended the socket after ${d[1]} ms`);
    is(await say('WSP free: #deaf'), 'released true');
  });

  // ---- the Notebook
  await page.click('#tab-notebook');
  await page.waitForFunction(() => /ready|idle/.test(document.getElementById('nb-status').dataset.state || ''), null, { timeout: 120000 });
  const nbDone = /^(ok|error|interrupted|cancelled)$/;
  const nbStart = async source => {
    await page.click('#nb-end-code');
    const i = (await page.$$eval('#nb-cells > li', l => l.length)) - 1;
    const src = page.locator('#nb-cells > li').nth(i).locator('.nb-src');
    await src.fill(source);
    await src.press('Control+Enter');
    return i;
  };
  const nbWait = (i, re, timeout = 60000) => page.waitForFunction(([i, s]) =>
    new RegExp(s).test((document.querySelectorAll('#nb-cells > li')[i] || { dataset: {} }).dataset.status || ''), [i, re.source], { timeout });
  const nbSay = async (source, timeout) => {
    const i = await nbStart(source);
    await nbWait(i, nbDone, timeout);
    const text = await page.$eval(`#nb-cells > li:nth-child(${i + 1}) .nb-out`, e => e.textContent);
    const m = /\[\[([^]*)\]\]/.exec(text);
    return m ? m[1].replace(/''/g, "'") : '!' + text;
  };

  await check('Notebook: a cell reads a push; a process waits for a message while other cells run', async () => {
    is(await nbSay(evalFile('/pharo/lane/websocket-spec.st')), 'ready');
    is(await nbSay(`WSP connect: #p url: '${W}/ws/push?n=3&close=none&t=nb'`), `2|||${W}/ws/push?n=3&close=none&t=nb`);
    is(await nbSay('WSP nextOf: #p'), '1');
    is(await nbSay(`WSP connect: #q url: '${W}/ws/echo?t=nbq'`), `2|||${W}/ws/echo?t=nbq`);
    is(await nbSay('WSP fork: #q'), 'forked');
    is(await nbSay('WSP nextOf: #p'), '2');
    is(await nbSay('WSP got'), 'got nil');
    is(await nbSay("WSP send: #q text: 'to the process'"), 'sent true');
    await waitFor('the process', async () => await nbSay('WSP got') === "got 'to the process'");
    is(await nbSay('WSP freeAll'), 'released all');
  });

  // ---- the setting of the network
  const nbMenu = async () => { await page.click('#nb-more'); await page.click('#nb-menu [data-act="settings"]'); };
  await check('the network off and on: Notebook, and the Console with it; another tab follows', async () => {
    await checkOn(nbSay, 'n1');
    await checkGoesOff(nbSay, () => set(page, false, nbMenu), 'n1');
    await checkOff(nbSay, 'n2');
    await page.click('#tab-console');
    await checkOff(say, 'n3');
    await set(page, true);
    await checkOn(say, 'n4');
    const tab = await newPage();
    await tab.goto(base + 'index.html');
    await waiting(tab);
    await prepare(tab);
    const tabSay = s => ev(tab, s);
    await checkOn(tabSay, 'n5');
    await checkGoesOff(tabSay, async () => {
      await set(page, false);
      await waitFor('the other tab', async () => await tabSay('WSP available') === 'availability 2');
    }, 'n5');
    await checkGoesOff(say, async () => {}, 'n4');
    await checkOff(tabSay, 'n6');
    await set(tab, true);
    await waitFor('this tab', async () => await say('WSP available') === 'availability 1');
    await checkOn(say, 'n7');
    is(await say('WSP free: #n7'), 'released true');
    await tab.close();
  });

  // ---- the world
  const entry = (manifest.images || []).find(e => e.default);
  if (!manifest.world) console.log('# skip the world: no world image');
  else {
    const w = await newPage();
    let n = 0;
    const request = source => w.evaluate(s => window.PharoWorld.fs('writeFile', '/pharo/git-eval.st', { data: s }), source);
    const answer = (k, ms = 60000) => waitFor('answer ' + k, async () => {
      const text = await w.evaluate(() => window.PharoWorld.readFile('/pharo/git-eval.out').then(d => new TextDecoder().decode(d), () => ''));
      const m = /^(\d+) ([\s\S]*)\n<end>\n$/.exec(text);
      return m && Number(m[1]) === k && { text: m[2] };
    }, ms);
    const worldSay = async (source, ms) => {
      await request(source);
      const { text } = await answer(++n, ms), m = /\[\[([^]*)\]\]/.exec(text);
      return m ? m[1].replace(/''/g, "'") : '!' + text;
    };
    const frames = () => w.evaluate(() => window.PharoWorld.stats.frames);
    const draws = async () => {
      const at = await frames(), box = await w.locator('canvas').first().boundingBox();
      await w.mouse.click(box.x + box.width * 0.9, box.y + box.height * 0.8, { button: 'right' });
      await w.waitForTimeout(700);
      await w.keyboard.press('Escape');
      await w.waitForTimeout(300);
      return (await frames()) - at;
    };
    await check('World: a reader process updates a morph through UIManager default defer:', async () => {
      await w.goto(base + 'world.html' + (entry ? '?fresh=' + entry.id : ''));
      await w.waitForFunction(() => window.PharoWorld && window.PharoWorld.stats.frames > 0, null, { timeout: 180000 });
      is(await worldSay(evalFile('/pharo/lane/websocket-spec.st'), 120000), 'ready');
      is(await worldSay(`WSP connect: #m url: '${W}/ws/echo?t=world'`), `2|||${W}/ws/echo?t=world`);
      is(await worldSay('WSP morph: #m'), 'morph');
      is(await worldSay("WSP send: #m text: 'to the morph'"), 'sent true');
      await waitFor('the morph', async () => await worldSay('WSP morphText') === 'to the morph');
      is(await worldSay('WSP close: #m code: 1000'), `4|||${W}/ws/echo?t=world|1000||1`);
      await waitFor('the morph', async () => await worldSay('WSP morphText') === 'closed 4');
      is(await worldSay('WSP free: #m'), 'released true');
    });

    await check('World: Stop of a wait for a message in the UI process opens a debugger, and the world draws again', async () => {
      is(await worldSay(`WSP connect: #u url: '${W}/ws/echo?t=ui'`), `2|||${W}/ws/echo?t=ui`);
      assert(await draws() > 0, 'it draws');
      is(await worldSay('WSP debugger'), 'debuggers 0');
      await request('WSP waitNext: #u');
      n++;
      await w.waitForTimeout(500);
      is(await draws(), 0, 'frozen: frames');
      await w.click('#stop');
      const said = await waitFor('the debugger', async () => {
        const s = await worldSay('WSP debugger', 30000);
        return s !== 'debuggers 0' && s;
      }, 15000);
      is(said, 'debuggers 1 true');
      assert(await draws() > 0, 'it draws again');
      is(await worldSay('WSP closeDebuggers'), 'debuggers 0');
      is(await worldSay("WSP send: #u text: 'after'"), 'sent true');
      is(await worldSay('WSP nextOf: #u'), 'after');
      is(await worldSay('WSP free: #u'), 'released true');
    });

    await check('World: the network off from the other page, with both sockets open, and on again', async () => {
      await checkOn(worldSay, 'w1');
      await checkGoesOff(worldSay, () => set(page, false), 'w1');
      await checkOff(worldSay, 'w2');
      await set(w, true);
      await checkOn(worldSay, 'w3');
      is(await worldSay('WSP freeAll'), 'released all');
      const csp = await w.evaluate(() => window.__csp);
      assert(csp.length === 0, 'CSP ' + csp.join());
      await w.close();
    });
    await waitFor('the Console', async () => await say('WSP available') === 'availability 1');
  }

  if (!packageDir) console.log('# skip the package: no WEB_FETCH_DIR');
  else {
    await check('the package: WebFetch-WebSocket-Tests in the Console, with the live tests', async () => {
      is(await ev(page, "(Metacello new baseline: 'WebFetch'; repository: 'tonel:///pharo/lane/web-fetch.st/src'; " +
        "load: #(Core Tests WebSocket WebSocketTests)) ifNotNil: [ '[[loaded]]' ]", 180000), 'loaded');
      const at = (await term(page)).length;
      await expecting(SOCKET_ERRORS, async () => {
        await ev(page, evalFile('/pharo/lane/web-fetch.st/tests/run-websocket.st') + ". '[[done]]'", 600000);
      });
      const text = (await term(page)).slice(at), lines = text.split('\n').filter(l => /^(WebFetch\S*:|failed|error|skipped) ?/.test(l));
      for (const l of lines) console.log('# ' + l);
      const m = lines.map(l => /^(\S+): (\d+) run, (\d+) failures, (\d+) errors, (\d+) skipped, (\w+)$/.exec(l))
        .find(m => m && m[1] === 'WebFetch-WebSocket-Tests');
      assert(m && Number(m[2]) > 60 && m[3] === '0' && m[4] === '0' && m[6] === 'ready', m ? m[0] : text.slice(-2000));
      assert(lines.filter(l => /^skipped/.test(l)).every(l => /NativeTest|EchoServerTest/.test(l)), 'skipped');
      is(await ev(page, "'[[' , WebFetchWebSocket openCount printString , ']]'"), '0');
    });
  }

  await check('measures: when Chromium ended TCP after the close that /ws/deaf never answered', async () => {
    assert(deafClosed, 'the close of /ws/deaf was made');
    await waitFor('the end of TCP', () => deafEnded, 120000);
    clearInterval(deafWatch);
    console.log(`# /ws/deaf: Chromium ended TCP ${((deafEnded - deafClosed) / 1000).toFixed(1)} s after the close`);
  });

  await check('no violation of the CSP in the Console', async () => {
    const csp = await page.evaluate(() => window.__csp);
    assert(csp.length === 0, csp.join());
  });
  await ctx.close();
  site.close();
  site.closeAllConnections();
  other.close();
  await web.close();
});
