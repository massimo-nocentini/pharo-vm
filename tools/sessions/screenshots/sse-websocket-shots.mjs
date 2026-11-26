// sse-websocket-shots.mjs - screenshots of the event sources (Server-Sent
// Events) and WebSockets of WebFetch in the three pages of a site: the
// Console (an event source read with next, a WebSocket echo of text and
// bytes, a close of the server), the Notebook (a forked reader, messages
// pushed by the server) and the World (a Playground, the Transcript fed by a
// started source and a WebSocket, an Inspector on the socket)
//
//   [OUT=<dir>] [SHOTS_IMAGE=batteries] PLAYWRIGHT_MODULE=... PLAYWRIGHT_BROWSERS_PATH=... \
//     node tools/sessions/screenshots/sse-websocket-shots.mjs WEB_DIR
//
// WEB_DIR is the web/ of a build whose image SHOTS_IMAGE (an id of its
// manifest; default batteries) has WebFetch with WebFetchEventSource and
// WebFetch-WebSocket loaded, e.g. a batteries image built with
// BATTERIES_WEBFETCH=tonel://<web-fetch.st>/src.  The servers are those of the
// tests (tests/wasm/lib/fetch-http.mjs, its /sse and /ws/* endpoints), reached as
// localhost while the site is 127.0.0.1, so the refusal of the site's own host
// does not apply.  The World is driven by tests/wasm/st/git-world.st, as
// git.spec.mjs drives it.  SHOTS_SRC is the checkout whose test servers and
// serve.mjs are used (default: this one; it must have the /sse and /ws/*
// endpoints, which came with Server-Sent Events and WebSocket).  It writes 1-console.png, 2-notebook.png and
// 3-world.png into OUT (default: the current directory) and prints the
// Console's transcript and the World's answers.
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
const WT = path.resolve(process.env.SHOTS_SRC || path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../..'));
if (!fs.existsSync(path.join(WT, 'tests/wasm/lib/ws-server.mjs')))
  throw new Error(`${WT} has no tests/wasm/lib/ws-server.mjs: SHOTS_SRC must name a checkout with the /sse and /ws/* endpoints`);
const WEB = path.resolve(process.argv[2] || path.join(WT, 'build-wasm', 'web'));
const OUT = path.resolve(process.env.OUT || '.'), IMAGE = process.env.SHOTS_IMAGE || 'batteries';
const pw = await import(process.env.PLAYWRIGHT_MODULE ? path.join(process.env.PLAYWRIGHT_MODULE, 'index.mjs') : 'playwright');
const chromium = pw.chromium ?? pw.default.chromium;
const { handler } = await import(WT + '/packaging/emscripten/tools/serve.mjs');
const { startFetchHttp } = await import(WT + '/tests/wasm/lib/fetch-http.mjs');
const web = await startFetchHttp();
const B = `http://localhost:${web.port}`, W = `ws://localhost:${web.port}`;
const files = [{ path: '/pharo/lane/git-world.st', data: fs.readFileSync(WT + '/tests/wasm/st/git-world.st', 'utf8') }];
const patch = `
;(function () { var D = self.PharoVMDriver, start = D.start, vmArgs = D.vmArgs, files = ${JSON.stringify(files)};
  D.vmArgs = function (mode) { var a = vmArgs.apply(this, arguments); return mode === 'world' ? a.concat(['st', '/pharo/lane/git-world.st']) : a; };
  D.start = function (c, o) { o.files = (o.files || []).concat(files); return start.call(this, c, o); }; })();`;
const files0 = handler(WEB), driver = fs.readFileSync(WEB + '/vm-driver.js', 'utf8') + patch;
const site = http.createServer((q, r) => {
  if (new URL(q.url, 'http://x').pathname === '/vm-driver.js') { r.writeHead(200, { 'Content-Type': 'text/javascript' }); return r.end(driver); }
  files0(q, r);
});
await new Promise(f => site.listen(0, '127.0.0.1', f));
const base = `http://127.0.0.1:${site.address().port}/`;
const browser = await chromium.launch();
const ctx = await browser.newContext({ viewport: { width: 1280, height: 800 }, deviceScaleFactor: 1 });
const page = await ctx.newPage();
page.on('dialog', d => d.accept().catch(() => {}));
const term = () => page.$eval('#term', e => e.textContent);
const waiting = () => page.waitForFunction(() => document.getElementById('status').dataset.state === 'waiting' &&
  /(^|\n)st> $/.test(document.getElementById('term').textContent), null, { timeout: 240000 });
const say = async src => { await page.fill('#line', src); await page.press('#line', 'Enter'); await page.waitForTimeout(300); await waiting(); };

// ---- Console
await page.goto(base + 'index.html?fresh=' + IMAGE);
await waiting();
await say(`| s | s := WebFetch eventSource: '${B}/sse?key=shot1&n=3&utf8=1&ms=300'. [ (1 to: 3) collect: [ :i | | e | e := s next. e lastEventId -> e data ] ] ensure: [ s close ]`);
await say(`| ws | ws := WebFetchWebSocket connect: '${W}/ws/echo'. ws sendText: 'ciao, è WebSocket'; sendBytes: #[1 2 3 255]. [ { ws protocol. ws next. ws next } ] ensure: [ ws close ]`);
await say(`| ws | ws := WebFetchWebSocket connect: '${W}/ws/close?code=4000&reason=bye'. [ ws next ] on: WebFetchWebSocketClosed do: [ :e | { e class name. ws closeCode. ws closeReason } ]`);
await page.screenshot({ path: OUT + '/1-console.png' });
console.log((await term()).slice(-2500));

// ---- Notebook
await page.click('#tab-notebook');
await page.waitForFunction(() => /ready|idle/.test(document.getElementById('nb-status').dataset.state || ''), null, { timeout: 180000 });
const cell = async src => {
  await page.click('#nb-end-code');
  const i = (await page.$$eval('#nb-cells > li', l => l.length)) - 1;
  const s = page.locator('#nb-cells > li').nth(i).locator('.nb-src');
  await s.fill(src); await s.press('Control+Enter');
  await page.waitForFunction(i => /^(ok|error|interrupted|cancelled)$/.test((document.querySelectorAll('#nb-cells > li')[i] || { dataset: {} }).dataset.status || ''), i, { timeout: 60000 });
};
await cell(`"Server-Sent Events: a forked reader collects the events"
| s got |
got := OrderedCollection new.
s := WebFetch eventSource: '${B}/sse?key=shot2&n=4&ms=150'.
s onEvent: [ :e | got add: e type -> e data ]; start.
(Delay forSeconds: 2) wait.
s close.
got`);
await cell(`"WebSocket: messages pushed by the server"
| ws |
ws := WebFetchWebSocket connect: '${W}/ws/push?n=3&close=none'.
[ (1 to: 3) collect: [ :i | ws next ] ] ensure: [ ws close ]`);
await page.waitForTimeout(500);
await page.locator('#nb-cells > li').last().scrollIntoViewIfNeeded();
await page.evaluate(() => window.scrollBy(0, 400));
await page.waitForTimeout(500);
await page.screenshot({ path: OUT + '/2-notebook.png' });

// ---- World
const w = await ctx.newPage();
await w.goto(base + 'world.html?fresh=' + IMAGE);
await w.waitForFunction(() => window.PharoWorld && window.PharoWorld.stats.frames > 0, null, { timeout: 240000 });
await w.waitForTimeout(8000);
let n = 0;
const worldSay = async src => {
  await w.evaluate(s => window.PharoWorld.fs('writeFile', '/pharo/git-eval.st', { data: s }), src);
  const k = ++n, t0 = Date.now();
  for (;;) {
    const text = await w.evaluate(() => window.PharoWorld.readFile('/pharo/git-eval.out').then(d => new TextDecoder().decode(d), () => ''));
    const m = /^(\d+) ([\s\S]*)\n<end>\n$/.exec(text);
    if (m && Number(m[1]) === k) return m[2];
    if (Date.now() - t0 > 60000) throw new Error('no answer ' + k);
    await new Promise(f => setTimeout(f, 200));
  }
};
const code = `| source ws |
"Server-Sent Events, read by a forked reader"
source := (WebFetch eventSource: '${B}/sse?key=world&n=3&until=6&ms=500')
	onEvent: [ :event | UIManager default defer: [
		Transcript show: 'event ', event lastEventId, ': ', event data; newLine ] ];
	start.
"A WebSocket, answered in the UI process"
ws := WebFetchWebSocket connect: '${W}/ws/echo'.
ws sendText: 'ciao, è WebSocket'.
Transcript show: 'echo: ', ws next; newLine.
ws inspect`;
console.log(await worldSay(`World submorphs do: [ :m | (m isKindOf: SystemWindow) ifTrue: [ m delete ] ]. Transcript clear. Transcript open. (Smalltalk tools workspace openContents: '${code.replace(/'/g, "''")}') ifNotNil: [ :p | p window ifNotNil: [ :win | win position: 30@40; extent: 650@330 ] ]. 'ok'`));
console.log(await worldSay(`Smalltalk compiler evaluate: '${code.replace(/'/g, "''")}'. 'ran'`));
await w.waitForTimeout(6000);
console.log(await worldSay(`| t i | t := World submorphs detect: [ :m | (m isKindOf: SystemWindow) and: [ m label includesSubstring: 'Transcript' ] ] ifNone: [ nil ]. t ifNotNil: [ t position: 700@40; extent: 540@330 ]. i := World submorphs detect: [ :m | (m isKindOf: SystemWindow) and: [ m label = 'Inspector' ] ] ifNone: [ nil ]. i ifNotNil: [ i position: 30@385; extent: 650@330 ]. (World submorphs select: [ :m | m isKindOf: SystemWindow ]) collect: [ :m | m label ]`));
await w.waitForTimeout(1500);
await w.screenshot({ path: OUT + '/3-world.png' });
await browser.close(); site.close(); web.close();
console.log('done');
