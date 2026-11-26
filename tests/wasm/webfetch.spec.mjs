// webfetch.spec.mjs - fetch for the image in the pages, in a browser
//
//   PLAYWRIGHT_MODULE=... [WEB_FETCH_DIR=...] node tests/wasm/webfetch.spec.mjs WEB_DIR
//
// The Console, the Notebook and the World of a build, against servers of the
// spec on other ports (other origins): tests/wasm/lib/fetch-http.mjs, and one
// that takes WebSockets and the requests of a git proxy.
//
// With the primitives of the WebFetchPlugin (tests/wasm/st/webfetch-spec.st):
// a request to another origin with CORS, and without; gzip; redirects,
// followed and not; the headers that a browser sends, User-Agent dropped and
// without a preflight, a header of one's own with one; no cookie and no
// referrer; the CSP of the page, which blocks the page's own fetch (the
// control) and not the worker's, with no other violation; Stop of a wait in
// the Console and in the Notebook, whose request the server sees end; in the
// world Stop while the UI process waits for a request: the world is frozen,
// then a debugger opens on that process and the world draws again, and the
// request ends when the debugger is closed.  The setting of the network, in each
// page: off, with a request and a socket open (the request fails, the socket
// closes), then fetch, git and sockets refused and nothing sent; on again;
// off from init; kept across a reload and not taken from the URL; another
// tab follows; without storage it holds for the page; without the git
// transport the dialog has no git group.  git only on a build with the
// smart-HTTP transport, the world only with a world image.
//
// With WEB_FETCH_DIR, a checkout of the package (web-fetch.st): both of its
// suites run in the Console, with the live tests of requests and of event
// sources as in a browser (but those of WebFetchEchoServerTest, which go to a
// public server, skip), and Stop is also of WebFetch, of an event source
// (do:) and, in the Console, of a ZnClient on it.
//
// The pages come from a server of the spec, which adds the files above to
// those of the VM, and not through a route of Playwright: with a route
// Playwright answers the preflights itself.

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

// The files added to those of every VM: {path, data}
function laneFiles(url) {
  const files = [
    { path: '/pharo/lane/webfetch-spec.st', data: fs.readFileSync(path.join(here, 'st', 'webfetch-spec.st'), 'utf8') },
    { path: '/pharo/lane/git-world.st', data: fs.readFileSync(path.join(here, 'st', 'git-world.st'), 'utf8') },
    { path: '/pharo/webfetch.lane', data: url + ' suite\n' },
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
  // webfetch.spec.mjs
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

// A server of WebSockets and of the requests of a git proxy, which counts
// what comes
async function startSockets() {
  const log = { sockets: 0, closed: 0, frames: 0, proxied: 0 };
  const open = new Set();
  const server = http.createServer((q, r) => {
    log.proxied++;
    r.writeHead(404, { 'Access-Control-Allow-Origin': '*', 'Access-Control-Allow-Headers': '*' });
    r.end();
  });
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
  return { log, port: server.address().port, url: 'http://127.0.0.1:' + server.address().port,
           close: () => { for (const s of open) s.destroy(); server.close(); } };
}

await run(async t => {
  const { check, assert, manifest } = t;
  await t.page.close();
  const web = await startFetchHttp(), other = await startSockets();
  const B = web.url, gitHttp = !!manifest.gitHttp;

  // ---- the site, with the files of the lane
  const files = handler(t.webDir), driver = fs.readFileSync(path.join(t.webDir, 'vm-driver.js'), 'utf8') + patch(laneFiles(B));
  const site = http.createServer((q, r) => {
    const p = new URL(q.url, 'http://x').pathname;
    const reply = (type, body) => { r.writeHead(200, { 'Content-Type': type, 'Cache-Control': 'no-cache' }); r.end(body); };
    if (p === '/vm-driver.js') return reply('text/javascript; charset=utf-8', driver);
    if (p === '/manifest.json')
      return reply('application/json', JSON.stringify(Object.assign({}, manifest, { gitProxyDefault: other.url + '/proxy/' })));
    files(q, r);
  });
  await new Promise(f => site.listen(0, '127.0.0.1', f));
  const base = `http://127.0.0.1:${site.address().port}/`;

  // ---- the pages, in a context without a route
  const context = async () => {
    const c = await t.browser.newContext({ viewport: { width: 1280, height: 800 } });
    await c.addInitScript(() => {
      // the violations of the CSP, and the ready of the workers
      window.__csp = [];
      document.addEventListener('securitypolicyviolation', e => window.__csp.push(e.violatedDirective + ' ' + e.blockedURI));
      window.__ready = [];
      const post = Worker.prototype.postMessage;
      Worker.prototype.postMessage = function (m, x) {
        if (!this.__heard) {
          this.__heard = true;
          this.addEventListener('message', e => { if (e.data && e.data.type === 'ready') window.__ready.push(e.data); });
        }
        return post.call(this, m, x);
      };
    });
    return c;
  };
  const ctx = await context();
  const newPage = async (c = ctx) => {
    const p = await c.newPage();
    t.watch(p);
    p.on('dialog', d => d.accept().catch(() => {}));
    return p;
  };
  // run f, whose errors of the console and of requests that match re are
  // asked for
  const expecting = async (re, f) => {
    const at = t.errors.length;
    try { return await f(); } finally {
      await new Promise(f => setTimeout(f, 300));
      for (let i = t.errors.length - 1; i >= at; i--) if (re.test(t.errors[i])) t.errors.splice(i, 1);
    }
  };
  const waitFor = async (what, pred, ms = 10000) => {
    const t0 = Date.now();
    for (;;) {
      const v = await pred();
      if (v) return v;
      if (Date.now() - t0 > ms) throw new Error('timed out waiting for ' + what);
      await new Promise(f => setTimeout(f, 50));
    }
  };
  const asked = tag => web.requests.filter(r => r.url.includes(tag));
  const closed = tag => waitFor('the end of ' + tag, () => asked(tag).length && asked(tag).every(r => r.closed));
  // Settings: the network on or off
  const set = async (p, on, menu) => {
    if (menu) await menu(); else await p.click('#settings');
    await p.setChecked('#network-enabled', on);
    await p.click('#settings-save');
    await p.waitForFunction(() => !document.getElementById('settings-dialog').open);
  };
  const stored = p => p.evaluate(() => localStorage.getItem('pharo-wasm.network'));

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
      const text = document.getElementById('term').textContent.slice(at);
      if (document.getElementById('status').dataset.state !== 'waiting' || !/\nst> $/.test(text)) return false;
      // (the last ones: the line itself is above)
      const m = /\[\[([^]*)\]\]'\n/.exec(text.slice(Math.max(0, text.lastIndexOf('[['))));
      return m ? m[1] : '!' + text;
    }, at, { timeout });
    return (await h.jsonValue()).replace(/''/g, "'");
  };
  const evalFile = file => `Smalltalk compiler evaluate: '${file}' asFileReference contents`;
  const prepare = async p => {
    const said = await ev(p, evalFile('/pharo/lane/webfetch-spec.st'));
    if (said !== 'ready') throw new Error('webfetch-spec.st: ' + said);
  };
  const is = (got, want, what = '') => assert(got === want, `${what} got ${JSON.stringify(got)}, want ${JSON.stringify(want)}`);
  // the three ways out, on and off
  const checkOn = async (say, p) => {
    is(await say('WFP available'), 'availability 1');
    assert((await say(`WFP fetch: 'GET ${B}/hello'`)).startsWith('3|200|OK|'), 'fetch');
    is(await say(`WFP socket: ${other.port}`), 'socket true');
    if (gitHttp) {
      const n = other.log.proxied;
      await expecting(/404|proxy/, async () => {
        const said = await say("WFP git: 'http://git.invalid/o/r.git'");
        assert(!/network access/.test(said) && other.log.proxied > n, said);
      });
    }
  };
  const checkOff = async say => {
    const n = web.requests.length, before = JSON.stringify(other.log);
    is(await say('WFP available'), 'availability 2');
    is(await say(`WFP fetch: 'GET ${B}/hello'`), 'unsupported operation');
    const socket = await say(`WFP socket: ${other.port}`);
    assert(/^socket [A-Z]\w+$/.test(socket), socket);
    if (gitHttp) is(await say("WFP git: 'http://git.invalid/o/r.git'"), 'git git http: network access is off in Settings');
    assert(web.requests.length === n && JSON.stringify(other.log) === before, 'nothing was sent: ' + JSON.stringify(other.log));
  };
  // off while a request and a socket are open (the socket of checkOn)
  const checkGoesOff = async (say, off, tag) => {
    is(await say(`WFP hang: 'GET ${B}/hang?${tag}'`), 'hang true');
    await waitFor('the request', () => asked(tag).length);
    await expecting(/hang|ERR_ABORTED/, async () => {
      await off();
      is(await say('WFP hung'), 'hung 4 off');
      await closed(tag);
    });
    await waitFor('the end of the sockets', () => other.log.closed === other.log.sockets);
    const frames = other.log.frames, sent = await say('WFP send');
    assert(other.log.frames === frames, 'nothing on the socket: ' + sent);
  };

  // ---- the Console
  const page = await newPage();
  await page.goto(base + 'index.html?network=off');
  await waiting(page);
  await prepare(page);
  const say = source => ev(page, source);
  const origin = base.slice(0, -1);

  await check('Console: another origin, with CORS; ready.network, and not from the URL', async () => {
    assert((await page.evaluate(() => window.__ready.map(r => r.network))).join() === 'true', 'ready');
    const said = await say(`WFP fetch: 'GET ${B}/hello'`);
    assert(/^3\|200\|OK\|.*\/hello\|cors\|0\|.*\|5\|hello$/.test(said) && /\|x-a: café\|/.test(said), said);
    const r = asked('/hello').pop();
    assert(r.origin === origin && !r.headers.referer && !r.headers.cookie, JSON.stringify(r));
  });

  await check('Console: without CORS a network error, and nothing of the answer', async () => {
    const said = await expecting(/nocors|CORS|ERR_FAILED/, () => say(`WFP fetch: 'GET ${B}/nocors'`));
    assert(/^4\|network\|[^|]+\|0\|$/.test(said), said);
    assert(asked('/nocors').length === 1, 'the request was made');
  });

  await check('Console: gzip from another origin, 204, HEAD', async () => {
    const said = await say(`WFP fetch: 'GET ${B}/gzip'`);
    assert(/\|20000\|abcabcabc/.test(said), said);
    assert(/^3\|204\|.*\|0\|$/.test(await say(`WFP fetch: 'GET ${B}/204'`)), '204');
    assert(/^3\|200\|.*\|0\|$/.test(await say(`WFP fetch: 'HEAD ${B}/hello'`)), 'HEAD');
  });

  await check('Console: redirects, followed and not', async () => {
    const said = await say(`WFP fetch: 'GET ${B}/redir'`);
    assert(/^3\|200\|OK\|[^|]*\/hello\|cors\|1\|.*\|5\|hello$/.test(said), said);
    // (Chromium reports the request that it does not make as aborted)
    const n = asked('/hello').length;
    const manual = await expecting(/hello .*ERR_ABORTED/, () => say(`WFP fetch: 'GET ${B}/redir?manual' flags: 1`));
    assert(/^4\|redirect\|/.test(manual) && asked('/hello').length === n, manual);
  });

  await check('Console: no preflight for User-Agent, one for a header of one\'s own; no cookie', async () => {
    await page.evaluate(() => { document.cookie = 'secret=1'; });
    const of = async (tag, headers) => {
      const said = await say(`WFP fetch: 'GET ${B}/echo?${tag}${headers}'`);
      assert(said.startsWith('3|200|'), said);
      return asked(tag);
    };
    let l = await of('t-ua', '|User-Agent: Pharo|Cookie: a=b|Referer: http://x/|Accept-Encoding: x');
    assert(l.length === 1 && !l[0].preflight && !/Pharo/.test(l[0].headers['user-agent']) && !l[0].headers.cookie &&
           !l[0].headers.referer, JSON.stringify(l));
    l = await of('t-own', '|X-Own: 1');
    assert(l.length === 2 && l[0].preflight && l[1].headers['x-own'] === '1' && !l[1].headers.cookie, JSON.stringify(l));
    l = await of('t-auth', '|Authorization: Bearer 1');
    assert(l.length === 2 && l[0].preflight && l[1].headers.authorization === 'Bearer 1', JSON.stringify(l));
    const said = await say(`WFP fetch: 'POST ${B}/echo?t-json|Content-Type: application/json'`);
    assert(/POST\|\/echo\?t-json\|application\/json\|\|.*\|2\|hi$/.test(said) && asked('t-json')[0].preflight, said);
  });

  await check('Console: what is no request, and a server that does not answer', async () => {
    for (const u of ['ftp://x/', 'data:,x', 'hello', 'http://a:b@127.0.0.1/', 'file:///etc/passwd', 'javascript:1'])
      assert(/^4\|request\|/.test(await say(`WFP fetch: 'GET ${u}'`)), u);
    await expecting(/ERR_|127\.0\.0\.1:1\//, async () =>
      assert(/^4\|network\|/.test(await say("WFP fetch: 'GET http://127.0.0.1:1/'")), 'refused'));
    await expecting(/trunc|ERR_/, async () =>
      assert(/^4\|body\|/.test(await say(`WFP fetch: 'GET ${B}/trunc'`)), 'trunc'));
  });

  await check('the CSP: it blocks the fetch of the page (the control), and no request of the image', async () => {
    assert((await page.evaluate(() => window.__csp)).length === 0, 'before: ' + await page.evaluate(() => window.__csp));
    const n = asked('control').length;
    const said = await expecting(/Content Security Policy|control|Refused to connect/, () =>
      page.evaluate(u => fetch(u).then(r => 'status ' + r.status, e => e.name), B + '/hello?control'));
    is(said, 'TypeError');
    await waitFor('the violation', () => page.evaluate(() => window.__csp.length));
    const csp = await page.evaluate(() => window.__csp.splice(0));
    assert(csp.length === 1 && /^connect-src /.test(csp[0]) && asked('control').length === n, csp.join());
  });

  const stop = async (source, tag) => {
    const at = await type(page, source);
    await waitFor('the request', () => asked(tag).length);
    await page.waitForTimeout(300);
    await expecting(/ERR_ABORTED|127\.0\.0\.1/, async () => {
      const t0 = Date.now();
      await page.click('#stop');
      await page.waitForFunction(at => /Interrupted\.\nst> $/.test(document.getElementById('term').textContent.slice(at)), at, { timeout: 5000 });
      console.log(`# ${tag}: ${Date.now() - t0} ms`);
      await closed(tag);
    });
  };
  await check('Console: Stop of a wait for a request, whose end the server sees', async () => {
    await stop(`WFP fetch: 'GET ${B}/hang?stop1'`, 'stop1');
    assert((await say(`WFP fetch: 'GET ${B}/hello'`)).startsWith('3|200|'), 'it goes on');
  });

  const loadPackage = ev => ev("(Metacello new baseline: 'WebFetch'; repository: 'tonel:///pharo/lane/web-fetch.st/src'; " +
    "load: #(Core Tests Zinc ZincTests)) ifNotNil: [ '[[loaded]]' ]", 120000);
  if (!packageDir) console.log('# skip the package: no WEB_FETCH_DIR');
  else {
    await check('the package: both suites, in the Console, as in a browser', async () => {
      is(await loadPackage((source, ms) => ev(page, source, ms)), 'loaded');
      assert(/browser/.test(await ev(page, "WebFetchLiveTest isBrowser: true. '[[browser]]'")), 'isBrowser');
      const at = (await term(page)).length;
      // (the errors that the live tests ask for: refused, truncated, aborted,
      // without CORS)
      await expecting(/CORS|ERR_|127\.0\.0\.1/, async () => {
        await ev(page, evalFile('/pharo/lane/web-fetch.st/tests/run-webfetch.st') + ". '[[done]]'", 300000);
        await page.waitForTimeout(500);
      });
      const text = (await term(page)).slice(at), lines = text.split('\n').filter(l => /^(WebFetch\S*:|failed|error|skipped) ?/.test(l));
      for (const l of lines) console.log('# ' + l);
      for (const name of ['WebFetch-Tests', 'WebFetch-Zinc-Tests']) {
        const m = lines.map(l => /^(\S+): (\d+) run, (\d+) failures, (\d+) errors, (\d+) skipped, (\w+)$/.exec(l)).find(m => m && m[1] === name);
        assert(m && Number(m[2]) > 40 && m[3] === '0' && m[4] === '0' && m[6] === 'ready', name + ': ' + (m && m[0]));
      }
      assert(lines.filter(l => /^skipped/.test(l)).every(l => /NativeTest|EchoServerTest/.test(l)), 'skipped');
    });
    await check('the package: the server sees a Stop of WebFetch, of an event source and of ZnClient end their requests', async () => {
      // (the tags are matched as substrings: stop5 and stop6 are the Notebook's)
      for (const [tag, source] of [['stop2', `WebFetch get: '${B}/hang?stop2'`],
                                   ['stop3', `(WebFetch request: '${B}/endless?stop3') stream: [ :s | [ s next: 100. (Delay forMilliseconds: 1) wait ] repeat ]`],
                                   ['stop4', `WebFetchZinc install. [ ZnClient new get: '${B}/hang?stop4' ] ensure: [ WebFetchZinc uninstall ]`],
                                   ['stop-sse', `(WebFetch eventSource: '${B}/sse?end=hang&key=stop-sse') do: [ :e | ]`]])
        await stop(source, tag);
      is(await ev(page, "'[[' , WebFetchCall openCount printString , ' ' , WebFetchEventSource openCount printString , ' ' , WebFetchZinc isInstalled printString , ']]'"), '0 0 false');
    });
  }

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
  const nbStop = async (source, tag) => {
    const i = await nbStart(source);
    await waitFor('the request', () => asked(tag).length);
    await page.waitForTimeout(300);
    await expecting(/ERR_ABORTED|127\.0\.0\.1/, async () => {
      await page.click('#nb-stop');
      await nbWait(i, /^interrupted$/, 5000);
      await closed(tag);
    });
  };

  await check('Notebook: a request, and Stop of a wait', async () => {
    is(await nbSay(evalFile('/pharo/lane/webfetch-spec.st')), 'ready');
    assert(/^3\|200\|.*\|5\|hello$/.test(await nbSay(`WFP fetch: 'GET ${B}/hello?nb'`)), 'fetch');
    await nbStop(`WFP fetch: 'GET ${B}/hang?stop5'`, 'stop5');
    assert(/^3\|200\|/.test(await nbSay(`WFP fetch: 'GET ${B}/hello'`)), 'it goes on');
    if (!packageDir) return;
    is(await loadPackage(nbSay), 'loaded');
    await nbStop(`WebFetch get: '${B}/hang?stop6'`, 'stop6');
    is(await nbSay("'[[' , (WebFetch get: '" + B + "/hello') contents , ']]'"), 'hello');
  });

  // ---- the setting of the network
  const nbMenu = async () => { await page.click('#nb-more'); await page.click('#nb-menu [data-act="settings"]'); };
  await check('the network off and on: Notebook, and the Console with it', async () => {
    await checkOn(nbSay);
    await checkGoesOff(nbSay, () => set(page, false, nbMenu), 'off1');
    is(await stored(page), 'off');
    await checkOff(nbSay);
    await page.click('#tab-console');
    await checkOff(say);
    await set(page, true);
    is(await stored(page), null);
    await checkOn(say);
    await checkGoesOff(say, () => set(page, false), 'off2');
    await checkOff(say);
  });

  await check('the network off: kept across a reload, off from init in the Console and in the Notebook', async () => {
    await page.reload();
    await waiting(page);
    is(await page.evaluate(() => window.__ready[0].network), false);
    await prepare(page);
    await checkOff(say);
    await page.click('#tab-notebook');
    await page.waitForFunction(() => /ready|idle/.test(document.getElementById('nb-status').dataset.state || ''), null, { timeout: 120000 });
    is(await nbSay(evalFile('/pharo/lane/webfetch-spec.st')), 'ready');
    await checkOff(nbSay);
    await page.click('#tab-console');
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
    // whether the world draws what the mouse does: its menu, opened and closed
    const draws = async () => {
      const at = await frames(), box = await w.locator('canvas').first().boundingBox();
      await w.mouse.click(box.x + box.width * 0.9, box.y + box.height * 0.8, { button: 'right' });
      await w.waitForTimeout(700);
      await w.keyboard.press('Escape');
      await w.waitForTimeout(300);
      return (await frames()) - at;
    };
    await check('World: off from init; on at once, and the other page with it', async () => {
      await w.goto(base + 'world.html' + (entry ? '?fresh=' + entry.id : ''));
      await w.waitForFunction(() => window.PharoWorld && window.PharoWorld.stats.frames > 0, null, { timeout: 180000 });
      is(await worldSay(evalFile('/pharo/lane/webfetch-spec.st'), 120000), 'ready');
      await checkOff(worldSay);
      await set(w, true);
      await checkOn(worldSay);
      await waitFor('the Console', async () => await say('WFP available') === 'availability 1');
      await checkOn(say);
    });

    await check('World: Stop while the UI process waits opens a debugger on it, and the world draws again', async () => {
      if (packageDir) is(await loadPackage(worldSay), 'loaded');
      assert(await draws() > 0, 'it draws');
      is(await worldSay('WFP debugger'), 'debuggers 0');
      await request(packageDir ? `WebFetch get: '${B}/hang?stop7'` : `WFP fetch: 'GET ${B}/hang?stop7'`);
      n++;
      await waitFor('the request', () => asked('stop7').length);
      await w.waitForTimeout(500);
      is(await draws(), 0, 'frozen: frames');
      const t0 = Date.now();
      await w.click('#stop');
      // (an evaluation may come between the interrupt and the window)
      const said = await waitFor('the debugger', async () => {
        const s = await worldSay('WFP debugger', 30000);
        return s !== 'debuggers 0' && s;
      }, 15000);
      console.log(`# Stop to the debugger: ${Date.now() - t0} ms, ${said}`);
      is(said, 'debuggers 1 true');
      assert(await draws() > 0, 'it draws again');
      assert(!asked('stop7')[0].closed, 'the request is open as long as the debugger is');
      await expecting(/ERR_ABORTED|127\.0\.0\.1/, async () => {
        is(await worldSay('WFP closeDebuggers'), 'debuggers 0');
        await closed('stop7');
      });
      assert(/^3\|200\|/.test(await worldSay(`WFP fetch: 'GET ${B}/hello'`)), 'it goes on');
    });

    await check('World: off at once from the other page, with a request and a socket open', async () => {
      is(await worldSay(`WFP socket: ${other.port}`), 'socket true');
      await checkGoesOff(worldSay, () => set(page, false), 'off3');
      await checkOff(worldSay);
      await set(w, true);
      assert(/^3\|200\|/.test(await worldSay(`WFP fetch: 'GET ${B}/hello'`)), 'on');
      const csp = await w.evaluate(() => window.__csp);
      assert(csp.length === 0, 'CSP ' + csp.join());
      await w.close();
    });
  }

  await check('without storage the setting holds for the page; without the git transport no git group', async () => {
    const c = await context();
    await c.addInitScript(() => Object.defineProperty(window, 'localStorage',
      { configurable: true, get() { throw new DOMException('no', 'SecurityError'); } }));
    const body = JSON.stringify(Object.assign({}, manifest, { git: false, gitHttp: false }));
    await c.route(u => u.pathname.endsWith('/manifest.json'), r => r.fulfill({ contentType: 'application/json', body }));
    const p = await newPage(c);
    await p.goto(base + 'index.html');
    await waiting(p);
    await prepare(p);
    await p.click('#settings');
    for (const id of ['#git-settings', '#git-proxy-about', '#git-proxy-state']) assert(await p.isHidden(id), id + ' is hidden');
    is(await p.evaluate(() => document.activeElement.id), 'network-enabled');
    // with the keyboard
    await p.keyboard.press('Space');
    await p.keyboard.press('Enter');
    await p.waitForFunction(() => !document.getElementById('settings-dialog').open);
    is(await ev(p, 'WFP available'), 'availability 2');
    await p.click('#settings');
    assert(!(await p.isChecked('#network-enabled')), 'the dialog shows it');
    await p.keyboard.press('Escape');
    await c.close();
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
