// pages-regress.spec.mjs - regression checks of the Console and world pages
//
// usage: node pages-regress.spec.mjs WEB_DIR
//
// A plain node script, like page.spec.mjs and world.spec.mjs, which it does
// not repeat: tests/wasm/lib/pw.mjs serves WEB_DIR and drives the pages in
// each browser named by BROWSERS (default "chromium"), with Playwright from
// PLAYWRIGHT_MODULE.  Run it by hand after a change of packaging/emscripten/web
// or packaging/emscripten/st:
//
//   PLAYWRIGHT_MODULE=... BROWSERS=chromium,firefox node tests/wasm/pages-regress.spec.mjs build-wasm/web
//
// The Console: its live region, keys of an input method, output while the
// page is hidden (no animation frames), the .changes stored soon after an
// evaluation, the questions before the page goes, Reset after a failed boot,
// two sites of one origin, a database connection that the browser closed,
// and a browser without DecompressionStream.  Its Notebook tab: a cell of 5
// MB of output while the page is hidden, and the autosaves of two sites of
// one origin.  The world, when WEB_DIR has
// it: its progress bar and live region, F6 and Tab, a refused clipboard
// write, and a world that does not open.  The checks that make a page fail
// on purpose do so in contexts of their own, whose errors are not counted.
// Exits with status 1 if any check fails.

import fs from 'node:fs';
import path from 'node:path';
import { run } from './lib/pw.mjs';

// The init script of every context: document.hidden as the spec says
// (__hide, __show), and no animation frames while hidden, as in a
// background tab; the texts of the live region and the values of the
// progress bar as they come
const INIT = () => {
  let hidden = false, queued = [];
  Object.defineProperty(Document.prototype, 'hidden', { configurable: true, get: () => hidden });
  Object.defineProperty(Document.prototype, 'visibilityState', { configurable: true,
                                                                 get: () => hidden ? 'hidden' : 'visible' });
  const raf = window.requestAnimationFrame.bind(window);
  window.requestAnimationFrame = f => raf(t => { if (hidden) queued.push(f); else f(t); });
  window.__hide = () => { hidden = true; document.dispatchEvent(new Event('visibilitychange')); };
  window.__show = () => {
    hidden = false;
    document.dispatchEvent(new Event('visibilitychange'));
    const q = queued;
    queued = [];
    for (const f of q) raf(f);
  };
  // from the start of the document, which the scripts of the page change
  // before DOMContentLoaded
  window.__said = [];
  window.__progress = [];
  let said = '', progress = null;
  new MutationObserver(() => {
    const live = document.getElementById('status-live'), bar = document.getElementById('loading');
    if (live && live.textContent !== said) window.__said.push(said = live.textContent);
    if (bar && bar.tagName === 'DIV' && bar.getAttribute('aria-valuenow') !== progress)
      window.__progress.push(progress = bar.getAttribute('aria-valuenow'));
  }).observe(document, { childList: true, characterData: true, attributes: true, subtree: true });
};

await run(async t => {
  const { page, check, assert, manifest, base } = t;
  await t.context.addInitScript(INIT);

  // ---- helpers on a Console page p
  const state = p => p.$eval('#status', e => e.dataset.state);
  const waitState = (p, st, timeout = 90000) =>
    p.waitForFunction(s => document.getElementById('status').dataset.state === s, st, { timeout });
  const term = p => p.$eval('#term', e => e.textContent);
  const started = async (p, timeout = 120000) => {
    await waitState(p, 'waiting', timeout);
    await p.waitForFunction(() => /(^|\n)st> $/.test(document.getElementById('term').textContent), null, { timeout });
  };
  async function evalIn(p, text, re, timeout = 60000) {
    const at = (await term(p)).length;
    await p.fill('#line', text);
    await p.press('#line', 'Enter');
    await p.waitForFunction(([src, at]) => new RegExp(src).test(document.getElementById('term').textContent.slice(at)) &&
                            document.getElementById('status').dataset.state === 'waiting', [re.source, at], { timeout });
  }
  const noticeSays = (p, re, timeout = 90000) =>
    p.waitForFunction(src => !document.getElementById('notice').hidden &&
                      new RegExp(src).test(document.getElementById('notice-text').textContent), re.source, { timeout });
  async function save(p) {
    await p.evaluate(() => { document.getElementById('notice-text').textContent = ''; });
    await p.click('#save');
    await noticeSays(p, /^The image is saved in this browser|^Saved, but/, 90000);
    const text = await p.textContent('#notice-text');
    await waitState(p, 'waiting');
    return text;
  }
  // the meta of the slot in the database name, from page p
  const meta = (p, name = 'pharo-wasm') => p.evaluate(name => new Promise((resolve, reject) => {
    const r = indexedDB.open(name, 1);
    r.onupgradeneeded = () => { if (!r.result.objectStoreNames.contains('files')) r.result.createObjectStore('files'); };
    r.onerror = () => reject(r.error);
    r.onsuccess = () => {
      const db = r.result, g = db.transaction('files').objectStore('files').get('meta');
      g.onsuccess = () => { db.close(); resolve(g.result || null); };
      g.onerror = () => { db.close(); reject(g.error); };
    };
  }), name);
  // a context of its own, whose errors do not count.  Its route turns the
  // HTTP cache off, as the one of pw.mjs does, so that later routes see
  // every request
  const fresh = async () => {
    const context = await t.browser.newContext({ viewport: { width: 1280, height: 800 } });
    await context.addInitScript(INIT);
    await context.route('**/*', r => r.continue());
    return context;
  };

  // ---- the Console

  await check('Console: the status pill is no live region; the live region says loading, starting, ready', async () => {
    await page.goto(base + 'index.html');
    await started(page);
    const pill = await page.$eval('#status', e => [e.getAttribute('role'), e.getAttribute('aria-live')]);
    assert(!pill[0] && !pill[1], 'the pill ' + JSON.stringify(pill));
    const said = await page.evaluate(() => window.__said);
    assert(JSON.stringify(said) === '["Loading Pharo","Starting Pharo","Pharo is ready"]', 'said ' + JSON.stringify(said));
    assert(fs.readFileSync(path.join(t.webDir, 'index.html'), 'utf8').includes('id="loading" aria-hidden="true"'),
           'the loading text of the log is hidden');
  });

  await check('Console: the theme button says the theme in its name', async () => {
    const label = () => page.getAttribute('#theme', 'aria-label');
    assert(await label() === 'Theme: System', 'at first ' + await label());
    await page.click('#theme');
    assert(await label() === 'Theme: Light', 'then ' + await label());
    await page.click('#theme');
    await page.click('#theme');
    assert(await label() === 'Theme: System', 'at last ' + await label());
  });

  await check('Console: the key that ends a composition evaluates nothing; Escape in one clears nothing', async () => {
    const before = await term(page);
    await page.fill('#line', "'abc");
    const key = init => page.$eval('#line', (e, init) => {
      const k = new KeyboardEvent('keydown', Object.assign({ bubbles: true, cancelable: true }, init));
      e.dispatchEvent(k);
      return k.defaultPrevented;
    }, init);
    assert(!(await key({ key: 'Enter', keyCode: 229, isComposing: false })), 'Enter of WebKit after compositionend');
    assert(!(await key({ key: 'Enter', keyCode: 13, isComposing: true })), 'Enter in a composition');
    assert(!(await key({ key: 'Escape', keyCode: 229, isComposing: true })), 'Escape in a composition');
    assert(!(await key({ key: 'ArrowUp', keyCode: 229, isComposing: true })), 'ArrowUp in a composition');
    assert(await page.inputValue('#line') === "'abc", 'the line ' + JSON.stringify(await page.inputValue('#line')));
    // (the image is busy on its own for a slice or two soon after its
    // first prompt, so the state is waited for, and the terminal compared)
    await page.waitForFunction(() => document.getElementById('status').dataset.state === 'waiting', null, { timeout: 5000 });
    assert(await term(page) === before, 'nothing was evaluated: ' + JSON.stringify((await term(page)).slice(before.length)));
    await page.fill('#line', '');
    await evalIn(page, '3 + 4', /\n7\nst> $/);
  });

  await check('Console: more than 1 MiB of output while the page is hidden: acked as it comes, the VM goes on', async () => {
    await page.click('#clear');
    await page.evaluate(() => window.__hide());
    try {
      await page.fill('#line', '1 to: 40000 do: [ :i | Transcript show: (i printPaddedWith: $0 to: 8) , ' +
                               '(String new: 62 withAll: $x); cr ]. #done');
      await page.press('#line', 'Enter');
      // the pill follows the worker, not the frames; and no frames come to
      // the waits of Playwright, which poll in them by default
      await page.waitForFunction(() => document.getElementById('status').dataset.state === 'waiting', null,
                                 { timeout: 60000, polling: 100 });
      await page.waitForFunction(() => /\n#done\nst> $/.test(document.getElementById('term').textContent) === false,
                                 null, { timeout: 1000, polling: 100 });
    } finally { await page.evaluate(() => window.__show()); }
    try {
      await page.waitForFunction(() => /\n00040000x{62}\n#done\nst> $/.test(document.getElementById('term').textContent),
                                 null, { timeout: 30000 });
      const c = (await term(page)).length;
      assert(c <= 2000000, 'the terminal holds ' + c + ' characters');
    } finally { await page.click('#clear'); }
  });

  await check('Console: the .changes is stored soon after an evaluation, and a reload asks first', async () => {
    const text = await save(page);
    assert(/^The image is saved in this browser/.test(text), 'saved: ' + text);
    const before = (await meta(page)).changesSize;
    await evalIn(page, "Object compile: 'pagesRegressProbe ^ 1' classified: 'regress'. #c", /\n#c\nst> $/);
    const t0 = Date.now();
    let stored = null;
    while (Date.now() - t0 < 4000 && !stored) {
      const m = await meta(page);
      if (m.changesSize > before) stored = Date.now() - t0;
      else await page.waitForTimeout(100);
    }
    assert(stored !== null && stored < 2500, 'the .changes stored ' + (stored === null ? 'not within 4 s' : 'after ' + stored + ' ms'));
    console.log('  # the .changes was stored ' + stored + ' ms after the prompt');
    // code that the saved image lacks: the browser asks before a reload
    const dialogs = [];
    const dialog = d => { dialogs.push(d.type()); d.accept(); };
    page.on('dialog', dialog);
    try {
      await page.reload();
      await started(page);
    } finally { page.off('dialog', dialog); }
    assert(dialogs.join() === 'beforeunload', 'dialogs ' + dialogs);
  });

  await check('Console: no question once the image is saved, nor after evaluations that change no code', async () => {
    await evalIn(page, "Object compile: 'pagesRegressProbe ^ 2' classified: 'regress'. #d", /\n#d\nst> $/);
    await save(page);
    await evalIn(page, '3 + 4', /\n7\nst> $/);
    const dialogs = [];
    const dialog = d => { dialogs.push(d.type()); d.accept(); };
    page.on('dialog', dialog);
    try {
      await page.reload();
      await started(page);
    } finally { page.off('dialog', dialog); }
    assert(!dialogs.length, 'dialogs ' + dialogs);
  });

  if (manifest.world) await check('Console: the world link offers to save first; the world then has the evaluation', async () => {
    assert(await page.isVisible('#world'), 'the world link');
    await evalIn(page, 'Smalltalk at: #PagesRegressMark put: 55', /\n55\nst> $/);
    await page.click('#world');
    await noticeSays(page, /lacks what you did here since it was saved/, 5000);
    const labels = await page.evaluate(() => ['notice-action', 'notice-alt'].map(id => document.getElementById(id))
                                         .filter(b => !b.hidden).map(b => b.textContent));
    assert(JSON.stringify(labels) === '["Save, then open the world","Open it without saving"]', 'buttons ' + JSON.stringify(labels));
    assert(/index\.html$/.test(page.url()), 'still on the Console');
    await page.click('#notice-action');
    await page.waitForURL(/world\.html$/, { timeout: 120000 });
    await page.waitForFunction(() => window.PharoWorld && window.PharoWorld.stats.frames > 0, null, { timeout: 120000 });
    await page.goto(base + 'index.html');
    await started(page);
    await evalIn(page, 'Smalltalk at: #PagesRegressMark ifAbsent: [ #none ]', /\n55\nst> $/);
  });

  await check('Console: Reset deletes the saved image also after a boot that could not fetch the manifest', async () => {
    const context = await fresh();
    try {
      const p = await context.newPage();
      await p.goto(base + 'index.html');
      await started(p);
      await evalIn(p, 'Smalltalk at: #ResetMark put: 7', /\n7\nst> $/);
      await save(p);
      await context.route('**/manifest.json*', r => r.fulfill({ status: 503, body: 'unavailable' }));
      await p.click('#restart');
      await waitState(p, 'crashed');
      assert(!(await p.isDisabled('#reset')), 'Reset enabled');
      p.once('dialog', d => d.accept());
      await p.click('#reset');
      await p.waitForFunction(() => /Deleted the image saved in this browser|Reset failed/.test(
        document.getElementById('term').textContent), null, { timeout: 30000 });
      assert(/Deleted the image saved in this browser/.test(await term(p)), 'term ' + JSON.stringify((await term(p)).slice(-300)));
      // the restart that follows fails too
      await p.waitForFunction(() => /Deleted the image saved in this browser[\s\S]*"The VM crashed"\n$/.test(
        document.getElementById('term').textContent), null, { timeout: 60000 });
      assert(await meta(p) === null, 'the slot is gone: ' + JSON.stringify(await meta(p)));
      await context.unroute('**/manifest.json*');
      await p.click('#restart');
      await started(p);
      await evalIn(p, 'Smalltalk at: #ResetMark ifAbsent: [ #none ]', /\n#none\nst> $/);
    } finally { await context.close(); }
  });

  await check('Console: two sites of one origin, /a/sub/ and /, keep an image each', async () => {
    const context = await fresh();
    try {
      await context.route(u => new URL(u).pathname.startsWith('/a/sub/'), async r => {
        const u = new URL(r.request().url());
        u.pathname = u.pathname.slice('/a/sub'.length);
        await r.fulfill({ response: await r.fetch({ url: u.href }) });
      });
      const a = await context.newPage();
      t.watch(a);
      await a.goto(base + 'a/sub/index.html');
      await started(a);
      await evalIn(a, 'Smalltalk at: #SiteMark put: 41', /\n41\nst> $/);
      await save(a);
      const b = await context.newPage();
      t.watch(b);
      await b.goto(base + 'index.html');
      await started(b);
      assert(!/Started the image saved in this browser/.test(await term(b)), 'the root site booted the other one');
      await evalIn(b, 'Smalltalk at: #SiteMark ifAbsent: [ #none ]', /\n#none\nst> $/);
      const m = await meta(b, 'pharo-wasm:/a/sub/');
      assert(m && m.imageSize > 1e7, 'the slot of /a/sub/ ' + JSON.stringify(m));
      assert(await meta(b) === null, 'the root has no slot');
      await a.close();
      await b.close();
    } finally { await context.close(); }
  });

  // ---- the Notebook tab of the Console page

  const nbState = (p, re, timeout = 90000) =>
    p.waitForFunction(([s, f]) => new RegExp(s, f).test(document.getElementById('nb-status').dataset.state),
                      [re.source, re.flags], { timeout, polling: 100 });
  // the source of the last code cell of p, as typed
  const setLast = (p, v) => p.evaluate(v => {
    const ta = [...document.querySelectorAll('#nb-cells > li[data-type="code"] .nb-src')].pop();
    ta.value = v;
    ta.dispatchEvent(new Event('input'));
  }, v);

  await check('Notebook: more than 1 MiB of output while the page is hidden: acked as it comes, the cell ends', async () => {
    const context = await fresh();
    try {
      const p = await context.newPage();
      t.watch(p);
      await p.goto(base + 'index.html');
      await started(p);
      await p.click('#tab-notebook');
      await nbState(p, /^ready$/);
      await p.click('#nb-end-code');
      const i = await p.$$eval('#nb-cells > li', l => l.length - 1);
      const src = p.locator('#nb-cells > li').nth(i).locator('.nb-src');
      await src.fill('1 to: 50000 do: [ :k | Transcript show: (k printPaddedWith: $0 to: 8) , (String new: 91 withAll: $x); cr ]. #done');
      await p.evaluate(() => window.__hide());
      const t0 = Date.now();
      try {
        await src.press('Control+Enter');
        // (no frames come to the waits of Playwright either: polled)
        await p.waitForFunction(i => document.querySelectorAll('#nb-cells > li')[i].dataset.status === 'ok', i,
                                { timeout: 120000, polling: 100 });
      } finally { await p.evaluate(() => window.__show()); }
      console.log('  # 5 MB in a hidden page in ' + (Date.now() - t0) + ' ms');
      await p.waitForFunction(i => {
        const li = document.querySelectorAll('#nb-cells > li')[i];
        return /00050000x{91}\s*$/.test([...li.querySelectorAll('.nb-stream')].map(e => e.textContent).join('')) &&
          [...li.querySelectorAll('.nb-value')].some(e => e.textContent === '#done');
      }, i, { timeout: 30000 });
      await p.close();
    } finally { await context.close(); }
  });

  await check('Notebook: two sites of one origin, /a/ and /a/sub/, keep an autosave each', async () => {
    const context = await fresh();
    try {
      await context.route(u => new URL(u).pathname.startsWith('/a/'), async r => {
        const u = new URL(r.request().url());
        u.pathname = u.pathname.slice(u.pathname.startsWith('/a/sub/') ? '/a/sub'.length : '/a'.length);
        await r.fulfill({ response: await r.fetch({ url: u.href }) });
      });
      const sites = { '/a/': null, '/a/sub/': null };
      for (const dir of Object.keys(sites)) {
        const p = sites[dir] = await context.newPage();
        t.watch(p);
        await p.goto(base.replace(/\/$/, '') + dir + 'index.html');
        await started(p);
        await p.click('#tab-notebook');
        await p.waitForFunction(() => document.querySelector('#nb-cells > li[data-type="code"]'));
      }
      const saved = (p, dir, re) => p.waitForFunction(([k, s]) => new RegExp(s).test(localStorage.getItem(k) || ''),
                                                      ['pharo-wasm.notebook:' + dir, re.source], { timeout: 5000 });
      for (const [dir, text] of [['/a/', "'site a'"], ['/a/sub/', "'site sub'"], ['/a/', "'site a again'"]]) {
        await setLast(sites[dir], text);
        await saved(sites[dir], dir, new RegExp(text));
      }
      await sites['/a/'].waitForTimeout(1500);
      const keys = await sites['/a/'].evaluate(() => Object.keys(localStorage).filter(k => /notebook/.test(k)).sort());
      assert(JSON.stringify(keys) === '["pharo-wasm.notebook:/a/","pharo-wasm.notebook:/a/sub/"]', 'keys ' + JSON.stringify(keys));
      const a = await sites['/a/'].evaluate(() => localStorage.getItem('pharo-wasm.notebook:/a/'));
      const sub = await sites['/a/sub/'].evaluate(() => localStorage.getItem('pharo-wasm.notebook:/a/sub/'));
      assert(/'site a again'/.test(a) && !/'site sub'/.test(a) && /'site sub'/.test(sub) && !/'site a/.test(sub), 'the autosaves mixed');
      for (const [dir, p] of Object.entries(sites)) {
        const n = await p.evaluate(() => [document.getElementById('nb-notice').hidden, document.getElementById('nb-notice-text').textContent]);
        assert(n[0] || !/another tab/.test(n[1]), dir + ' asks: ' + n[1]);
        await p.close();
      }
    } finally { await context.close(); }
  });

  if (t.name === 'chromium') await check('Console: a database connection that the browser closed is opened again', async () => {
    const context = await fresh();
    try {
      const p = await context.newPage();
      t.watch(p);
      await p.goto(base + 'index.html');
      await started(p);
      assert(/^The image is saved in this browser/.test(await save(p)), 'the first save');
      const cdp = await context.newCDPSession(p);
      await cdp.send('Storage.clearDataForOrigin', { origin: new URL(base).origin, storageTypes: 'indexeddb' });
      await p.waitForTimeout(500);
      const text = await save(p);
      assert(/^The image is saved in this browser/.test(text), 'the second save: ' + text);
      assert((await meta(p)).imageSize > 1e7, 'stored');
      await p.close();
    } finally { await context.close(); }
  });

  await check('without DecompressionStream the pages say so instead of crashing', async () => {
    const context = await fresh();
    try {
      await context.addInitScript(() => { delete window.DecompressionStream; });
      const p = await context.newPage();
      await p.goto(base + 'index.html');
      await waitState(p, 'error', 10000);
      assert(/DecompressionStream/.test(await p.textContent('#notice-text')), 'Console: ' + await p.textContent('#notice-text'));
      if (manifest.world) {
        await p.goto(base + 'world.html');
        await waitState(p, 'error', 10000);
        assert(/DecompressionStream/.test(await p.textContent('#notice-text')), 'world: ' + await p.textContent('#notice-text'));
        assert(await p.isHidden('#notice-link'), 'no link to a Console that cannot run either');
      }
      await p.close();
    } finally { await context.close(); }
  });

  if (!manifest.world) { console.log('  # no world image: the checks of the world page are skipped'); return; }

  // ---- the world

  // keeps the messages that world.js posts to its worker, and lets the spec
  // post one to the page as the worker would
  const WORKERS = () => {
    window.__posted = [];
    const W = window.Worker;
    window.Worker = class extends W {
      constructor(...a) { super(...a); window.__worker = this; }
      postMessage(m, transfer) { window.__posted.push(m && m.type === 'display' ? { kind: m.kind, text: m.text } : null); return super.postMessage(m, transfer); }
    };
  };
  const framed = p => p.waitForFunction(() => window.PharoWorld && window.PharoWorld.stats.frames > 0, null, { timeout: 120000 });

  await check('world: the progress bar has values, the live region says loading, starting, running', async () => {
    const context = await fresh();
    try {
      const p = await context.newPage();
      t.watch(p);
      await p.goto(base + 'world.html');
      await framed(p);
      const values = (await p.evaluate(() => window.__progress)).filter(v => v !== null).map(Number);
      assert(values.length >= 5 && values.every(v => v >= 0 && v <= 100) && Math.max(...values) >= 99,
             'aria-valuenow ' + JSON.stringify(values.slice(0, 10)) + '...');
      await p.waitForFunction(() => document.getElementById('status-live').textContent === 'The Pharo world is running');
      const said = await p.evaluate(() => window.__said);
      assert(JSON.stringify(said) === '["Loading Pharo","Starting Pharo","The Pharo world is running"]', 'said ' + JSON.stringify(said));
      const pill = await p.$eval('#status', e => [e.getAttribute('role'), e.getAttribute('aria-live')]);
      assert(!pill[0] && !pill[1], 'the pill ' + JSON.stringify(pill));

      // F6 leaves the world for the toolbar; Tab goes back into it
      await p.waitForFunction(() => document.activeElement && document.activeElement.id === 'sink');
      await p.keyboard.press('F6');
      assert(await p.evaluate(() => document.activeElement.id) === 'stop', 'F6 went to ' + await p.evaluate(() => document.activeElement.id));
      let at = '';
      for (let i = 0; i < 6 && at !== 'sink'; i++) {
        await p.keyboard.press('Tab');
        at = await p.evaluate(() => document.activeElement.id);
      }
      assert(at === 'sink', 'Tab went to ' + at);
      await p.close();
    } finally { await context.close(); }
  });

  await check('world: after a refused clipboard write, a paste keeps the image\'s copy, until the page lost the focus', async () => {
    const context = await fresh();
    try {
      await context.addInitScript(WORKERS);
      await context.addInitScript(() => {
        Object.defineProperty(navigator, 'clipboard', { configurable: true, value: {
          writeText: () => Promise.reject(new DOMException('refused', 'NotAllowedError')) } });
      });
      const p = await context.newPage();
      t.watch(p);
      await p.goto(base + 'world.html');
      await framed(p);
      // (a paste event of the page's own: Firefox drops the clipboardData
      // given to the constructor of a ClipboardEvent)
      const paste = () => p.evaluate(() => {
        const n = window.__posted.length, e = new Event('paste', { bubbles: true, cancelable: true });
        Object.defineProperty(e, 'clipboardData', { value: { getData: type => type === 'text/plain' ? 'OLD SYSTEM TEXT' : '' } });
        document.getElementById('sink').dispatchEvent(e);
        return window.__posted.slice(n).filter(m => m && m.kind === 'clipboard').map(m => m.text);
      });
      assert(JSON.stringify(await paste()) === '["OLD SYSTEM TEXT"]', 'a paste before any copy');
      await p.evaluate(() => window.__worker.onmessage({ data: { type: 'display', kind: 'clipboardSet', text: 'COPY' } }));
      await noticeSays(p, /pastes within Pharo only/, 5000);
      assert(JSON.stringify(await paste()) === '[]', 'the old text came over the copy');
      await p.evaluate(() => window.dispatchEvent(new Event('blur')));
      assert(JSON.stringify(await paste()) === '["OLD SYSTEM TEXT"]', 'a paste after the page lost the focus');
      await p.close();
    } finally { await context.close(); }
  });

  await check('world: a VM that ends before the world painted points to the Console', async () => {
    const context = await fresh();
    try {
      await context.route('**/manifest.json*', r => r.fulfill({ status: 503, body: 'unavailable' }));
      const p = await context.newPage();
      await p.goto(base + 'world.html');
      await p.waitForFunction(() => /^(crashed|exited)$/.test(document.getElementById('status').dataset.state), null,
                              { timeout: 60000 });
      assert(await p.textContent('#notice-action') === 'Restart' && await p.isVisible('#notice-alt') &&
             await p.getAttribute('#notice-alt', 'href') === 'index.html' &&
             await p.textContent('#notice-alt') === 'Open the Console', 'the notice ' + await p.textContent('#notice-text'));
      await p.close();
    } finally { await context.close(); }
  });

  const stock = path.join(t.webDir, '..', 'image', 'stock');
  const stockImage = fs.existsSync(stock) && fs.readdirSync(stock).find(f => f.endsWith('.image'));
  if (stockImage) await check('world: an image said prepared that does not open the world points to the Console', async () => {
    // the slot of a stock image, as a byte scan that a string fooled saved it
    const context = await fresh();
    try {
      const files = { '/__stock/image': path.join(stock, stockImage),
                      '/__stock/changes': path.join(stock, stockImage.replace(/\.image$/, '.changes')) };
      await context.route(u => new URL(u).pathname in files || new URL(u).pathname === '/__blank', r => {
        const at = new URL(r.request().url()).pathname;
        if (at === '/__blank') return r.fulfill({ contentType: 'text/html', body: '<!doctype html><title>blank</title>' });
        return r.fulfill({ contentType: 'application/octet-stream', body: fs.readFileSync(files[at]) });
      });
      const p = await context.newPage();
      await p.goto(base + '__blank');
      await p.evaluate(async () => {
        const [image, changes] = await Promise.all(['/__stock/image', '/__stock/changes'].map(u => fetch(u).then(r => r.blob())));
        const db = await new Promise((resolve, reject) => {
          const r = indexedDB.open('pharo-wasm', 1);
          r.onupgradeneeded = () => r.result.createObjectStore('files');
          r.onsuccess = () => resolve(r.result);
          r.onerror = () => reject(r.error);
        });
        await new Promise((resolve, reject) => {
          const tx = db.transaction('files', 'readwrite'), s = tx.objectStore('files');
          s.put(image, 'Pharo.image');
          s.put(changes, 'Pharo.changes');
          s.put({ id: 'stock-prepared', image: 'Pharo.image', changes: 'Pharo.changes', imageSize: image.size,
                  changesSize: changes.size, savedAt: Date.now(), syncedAt: Date.now(), prepared: true }, 'meta');
          tx.oncomplete = resolve;
          tx.onabort = () => reject(tx.error);
        });
        db.close();
      });
      await p.goto(base + 'world.html');
      await p.waitForFunction(() => !document.getElementById('notice').hidden &&
                              (/has not opened yet/.test(document.getElementById('notice-text').textContent) ||
                               !document.getElementById('notice-alt').hidden), null, { timeout: 60000 });
      const link = await p.evaluate(() => [...document.querySelectorAll('#notice a')].filter(a => !a.hidden)
                                      .map(a => [a.textContent, a.getAttribute('href')]));
      assert(JSON.stringify(link).includes('["Open the Console","index.html"]'), 'links ' + JSON.stringify(link));
      await p.close();
    } finally { await context.close(); }
  });
});
