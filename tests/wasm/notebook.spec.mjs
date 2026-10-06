// notebook.spec.mjs - browser test of the Notebook tab of the Console page
//
// usage: node notebook.spec.mjs WEB_DIR   (see tests/wasm/lib/pw.mjs)
//
// A plain node script, run by `make wasm-check-browser' after page.spec.mjs,
// and never a build dependency.  tests/wasm/lib/pw.mjs serves WEB_DIR and
// drives the page in each browser named by BROWSERS (default "chromium"),
// each in a context of its own, so that its IndexedDB and localStorage start
// empty.  The notebook (notebook.js, nb-kernel.js) runs a second VM, the
// kernel (st/web-notebook.st), in a worker of its own, which the first visit
// to the tab starts; tests/wasm/notebook-harness.js checks that kernel in
// node, this spec the page around it:
//
//   - the tabs, their keys and their toolbars, and the lazy start;
//   - Run all on the example notebook, which stops at its error, and the
//     rich outputs of the example (a table, an SVG in currentColor, a
//     progress bar updated in place, a Form as a PNG);
//   - the keys that run a cell, the order of stdout and stderr, a kernel
//     separate from the Console's VM, Stop (the button, i i, Ctrl+C), the
//     Stop watchdog (a primitive that never yields, after a Delay, and a
//     valueUnpreemptively loop that the watcher cannot reach), output that
//     streams while a cell runs, a syntax error that runs nothing;
//   - the coloring of the code cells: every kind of token, an overlay that
//     is inert and lines up with the editor, an IME, the repaint of the
//     lines that changed only, a cell of 32 KB, text cells left plain, the
//     Smalltalk fences of Markdown colored lazily within their budget, the
//     contrast of every color, and the highlighter itself (a fuzz against
//     the bracket scanner, linear time);
//   - the caps of the outputs, the sanitizer of html:, svg: and markdown:,
//     of Markdown cells and of imported notebooks, the bounds of <use> and
//     of filters, Markdown in linear time;
//   - the command keys, Upload, a drop on the tab, the theme and Settings
//     without a restart, layouts at 390, 360 and 320 px, the shortcuts
//     dialog, accessible names;
//   - Restart, export and import of .st and .json, the import limits, Load
//     theirs across tabs, autosave across a reload and when it fails, Ctrl+C
//     on a selection;
//   - Export .html: a static page with a CSP and no script, opened from
//     file:// (no requests, no console errors), with every kind of cell
//     and output, hostile markup kept in its cell or left out with a note,
//     dark, print and phone layouts, and its coloring budget of 1 MB;
//   - the image of the Console: a Save, then a Restart of the kernel, which
//     boots it (and fetches no image from the server), a snapshot in a cell
//     that leaves the saved image as it was, the notice after a Save and a
//     Reset; the memory of the two VMs is logged;
//   - from file:// and without memory64, the notebook starts no worker;
//   - no Content-Security-Policy violation on any page, no worker warning
//     of an engine error.
//
// NB_OPEN_ZIP names a Pharo zip (such as build-wasm/images/p15/pharo15.zip)
// that the Console opens last: the kernel then boots it, and Run all on the
// example passes on it too.

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { run } from './lib/pw.mjs';

// the Content-Security-Policy of index.html, as the plan of the page has it
const CSP = "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self' 'unsafe-inline'; " +
  "img-src 'self' data: blob:; connect-src 'self'; worker-src 'self'; object-src 'none'; base-uri 'none'; " +
  "form-action 'none'; frame-src 'none'";
// what the workers warn of must be no engine error, nor a callback that threw
const ENGINE_ERRORS = /RuntimeError|RangeError|Aborted|unreachable|signature_mismatch|unsupported syscall|^vm-driver: /;
// a primitive that runs for seconds without a check for interrupts (page.spec.mjs's)
const LONG_PRIMITIVE = "(String new: 1000000 withAll: $a) findString: (String new: 8000 withAll: $a), 'b' startingAt: 1";
const NB_DONE = /^(ok|error|interrupted|cancelled)$/;
// a Smalltalk string literal of s
const st = s => "'" + s.replace(/'/g, "''") + "'";
// the kinds of the highlighter and their colors (index.html's --syn-*)
const SYN_KINDS = ['comment', 'string', 'char', 'number', 'quote', 'constant', 'special', 'keyword', 'paren', 'global'];
const synVar = k => '--syn-' + (k === 'char' ? 'string' : k);

// The init script of every context: the violations of the CSP, and the
// workers with what the page tells them of their mode and of the git proxy
const INIT = () => {
  document.addEventListener('securitypolicyviolation', e => {
    const said = e.violatedDirective + ' ' + (e.blockedURI || '') + ' at ' + location.href;
    if (window.__nbCsp) window.__nbCsp(said);
  });
  window.__posts = [];
  const W = window.Worker;
  let n = 0;
  window.Worker = class extends W {
    constructor(...a) { super(...a); this.__n = n++; }
    postMessage(m, ...rest) {
      if (m && (m.type === 'init' || m.type === 'gitProxy'))
        window.__posts.push({ worker: this.__n, type: m.type, mode: m.mode || 'console', gitProxy: m.gitProxy });
      return super.postMessage(m, ...rest);
    }
  };
};

// files dropped on the element of selector, as from the desktop: whether
// the page showed its #drop element, took the files and hid #drop again
const dropOn = (p, selector, files) => p.evaluate(([selector, files]) => {
  const data = new DataTransfer();
  for (const f of files) data.items.add(new File([f.data], f.name, { type: 'text/plain' }));
  const target = document.querySelector(selector);
  const fire = type => target.dispatchEvent(new DragEvent(type, { dataTransfer: data, bubbles: true, cancelable: true }));
  fire('dragenter');
  const drop = document.getElementById('drop'), shown = !drop.hidden;
  fire('dragover');
  const took = !fire('drop');
  return { shown, took, hidden: drop.hidden };
}, [selector, files]);

await run(async t => {
  const { page, check, assert, manifest, base } = t;
  const imageUrl = '/' + manifest.files.find(f => f.path === manifest.image).url;
  const changesUrl = '/' + manifest.files.find(f => f.path === manifest.image.replace(/\.image$/, '.changes')).url;
  const sourcesUrl = '/' + manifest.files.find(f => f.path.endsWith('.sources')).url;
  const NB_KEY = 'pharo-wasm.notebook:' + new URL(base).pathname;
  const csp = [], diag = [];
  await t.context.exposeFunction('__nbCsp', s => { csp.push(s); });
  await t.context.addInitScript(INIT);
  const listen = p => p.on('worker', w => w.on('console', m => { if (ENGINE_ERRORS.test(m.text())) diag.push(m.text()); }));
  listen(page);
  t.context.on('page', listen);

  // ---- the Console of a page p

  const term = (p = page) => p.$eval('#term', e => e.textContent);
  const waitState = (st, p = page, timeout = 90000) =>
    p.waitForFunction(s => document.getElementById('status').dataset.state === s, st, { timeout });
  const started = async (p = page, timeout = 120000) => {
    await waitState('waiting', p, timeout);
    await p.waitForFunction(() => /(^|\n)st> $/.test(document.getElementById('term').textContent), null, { timeout });
  };
  async function evalIn(p, text, re, timeout = 60000) {
    const at = (await term(p)).length;
    await p.fill('#line', text);
    await p.press('#line', 'Enter');
    await p.waitForFunction(([src, at]) => new RegExp(src).test(document.getElementById('term').textContent.slice(at)) &&
                            document.getElementById('status').dataset.state === 'waiting', [re.source, at], { timeout });
    return (await term(p)).slice(at);
  }
  const noticeSays = (re, p = page, timeout = 90000) =>
    p.waitForFunction(src => !document.getElementById('notice').hidden &&
                      new RegExp(src).test(document.getElementById('notice-text').textContent), re.source, { timeout });
  // the meta of the image saved in this browser (vm-storage.js), or null
  const slotMeta = (p = page) => p.evaluate(() => new Promise((resolve, reject) => {
    const r = indexedDB.open('pharo-wasm', 1);
    r.onupgradeneeded = () => { if (!r.result.objectStoreNames.contains('files')) r.result.createObjectStore('files'); };
    r.onerror = () => reject(r.error);
    r.onsuccess = () => {
      const db = r.result, g = db.transaction('files').objectStore('files').get('meta');
      g.onsuccess = () => { db.close(); resolve(g.result || null); };
      g.onerror = () => { db.close(); reject(g.error); };
    };
  }));
  const vmWorkers = (p = page) => p.workers().filter(w => /vm-worker\.js/.test(w.url())).length;
  const posts = (p = page) => p.evaluate(() => window.__posts);
  const notebookInits = async (p = page) => (await posts(p)).filter(m => m.type === 'init' && m.mode === 'notebook').length;
  async function accepting(f, p = page) {           // run f, accepting the confirm() dialogs
    const h = d => d.accept().catch(() => {});
    p.on('dialog', h);
    try { await f(); await p.waitForTimeout(200); } finally { p.off('dialog', h); }
  }
  // e is shown: laid out, and not hidden by visibility
  const shownIn = (p, sel) => p.evaluate(sel => {
    const e = document.querySelector(sel);
    return !!e && e.getClientRects().length > 0 && getComputedStyle(e).visibility !== 'hidden';
  }, sel);

  // ---- the Notebook of a page p

  const nbOf = p => {
    const o = {};
    o.cells = () => p.$$eval('#nb-cells > li', l => l.length);
    o.cell = i => p.locator('#nb-cells > li').nth(i);
    o.src = i => o.cell(i).locator('.nb-src');
    o.wait = (i, re = NB_DONE, timeout = 30000) =>
      p.waitForFunction(([i, s, f]) => {
        const li = document.querySelectorAll('#nb-cells > li')[i];
        return !!li && new RegExp(s, f).test(li.dataset.status);
      }, [i, re.source, re.flags], { timeout });
    o.status = i => p.$eval(`#nb-cells > li:nth-child(${i + 1})`, e => e.dataset.status);
    o.out = i => p.$eval(`#nb-cells > li:nth-child(${i + 1}) .nb-out`, e => e.textContent);
    o.values = i => p.$$eval(`#nb-cells > li:nth-child(${i + 1}) .nb-value`, l => l.map(e => e.textContent));
    o.count = i => p.$eval(`#nb-cells > li:nth-child(${i + 1}) .nb-count`, e => e.textContent);
    o.waitState = (re, timeout = 90000) =>
      p.waitForFunction(([s, f]) => new RegExp(s, f).test(document.getElementById('nb-status').dataset.state),
                        [re.source, re.flags], { timeout });
    o.add = async src => {              // a new code cell at the end; its index
      await p.click('#nb-end-code');
      const i = (await o.cells()) - 1;
      if (src != null) await o.src(i).fill(src);
      return i;
    };
    o.run = async (i, src, key = 'Control+Enter') => {
      if (src != null) await o.src(i).fill(src);
      await o.src(i).press(key);
    };
    o.eval = async (src, timeout) => {  // src run in a new cell, to its end
      const i = await o.add(src);
      await o.run(i);
      await o.wait(i, NB_DONE, timeout);
      return i;
    };
    o.menu = async act => {
      await p.click('#nb-more');
      await p.click(`#nb-menu [data-act="${act}"]`);
    };
    o.notice = () => p.textContent('#nb-notice-text');
    o.noticeSays = (re, timeout = 10000) =>
      p.waitForFunction(src => !document.getElementById('nb-notice').hidden &&
                        new RegExp(src).test(document.getElementById('nb-notice-text').textContent), re.source, { timeout });
    return o;
  };
  const nb = nbOf(page);

  // Run all on the example: the code cells up to its deliberate error ran
  // in order and are ok, the error is an index out of bounds, the cells
  // after it did not run
  async function runExample(p) {
    const types = await p.$$eval('#nb-cells > li', l => l.map(e => e.dataset.type));
    assert(types.length >= 8 && types[0] === 'markdown' && types.includes('code'), 'example loaded: ' + types);
    await p.click('#nb-run-all');
    await p.waitForFunction(() => {
      const l = [...document.querySelectorAll('#nb-cells > li[data-type="code"]')];
      return l.length && l.every(e => /^(ok|error|cancelled|interrupted)$/.test(e.dataset.status));
    }, null, { timeout: 120000 });
    const cells = await p.$$eval('#nb-cells > li[data-type="code"]', l => l.map(e =>
      ({ st: e.dataset.status, n: e.querySelector('.nb-count').textContent, out: e.querySelector('.nb-out').textContent.slice(0, 300),
         err: (e.querySelector('.nb-error-msg') || {}).textContent })));
    const bad = cells.findIndex(c => c.st !== 'ok');
    assert(bad > 0 && cells[bad].st === 'error', 'an error cell: ' + JSON.stringify(cells));
    assert(/SubscriptOutOfBounds|out of bounds/i.test(cells[bad].err), 'error text ' + cells[bad].err);
    cells.slice(0, bad + 1).forEach((c, k) => assert(c.n === '[' + (k + 1) + ']', 'count of code cell ' + k + ': ' + c.n));
    assert(cells.slice(bad + 1).length && cells.slice(bad + 1).every(c => c.st === 'cancelled'),
           'cells after it not run: ' + JSON.stringify(cells.slice(bad + 1)));
    return cells;
  }

  await page.goto(base + 'index.html');

  await check('the kernel starts on the first visit to the tab: one VM worker before it, two after', async () => {
    await started();
    assert(vmWorkers() === 1, 'one VM worker before the first visit: ' + vmWorkers());
    assert(await page.$eval('#nb-status', e => e.dataset.state) === 'off', 'the kernel is not started');
    assert(await notebookInits() === 0, 'no notebook worker yet');
    await page.evaluate(() => {
      window.__nbStates = [];
      const s = document.getElementById('nb-status');
      new MutationObserver(() => window.__nbStates.push(s.dataset.state)).observe(s, { attributes: true });
    });
    const n = t.requests.length, t0 = Date.now();
    await page.click('#tab-notebook');
    await nb.waitState(/^ready$/);
    const ms = Date.now() - t0;
    const states = await page.evaluate(() => window.__nbStates);
    assert(states.indexOf('loading') >= 0 && states.indexOf('loading') < states.lastIndexOf('ready'), 'states ' + states);
    assert(/^Pharo 1[25]\.\d+ \u00b7 wasm64$/.test(await page.textContent('#nb-info')), 'info ' + await page.textContent('#nb-info'));
    assert(vmWorkers() === 2, 'two VM workers after it: ' + vmWorkers());
    assert(await notebookInits() === 1, 'one notebook init: ' + JSON.stringify(await posts()));
    // without an image saved in this browser, the kernel fetches the one of
    // the site again: how often reaches the server is logged, not asserted
    // (the HTTP cache of workers differs between browsers)
    const served = t.requests.slice(n);
    console.log(`  # the kernel was ready ${ms} ms after the visit; it fetched the image ` +
                served.filter(u => u === imageUrl).length + ' time(s), the .sources ' +
                served.filter(u => u === sourcesUrl).length + ' time(s)');
  });

  await check('tabs Console | Notebook: arrow keys, Home and End; a toolbar each; the Console\'s status only on its tab', async () => {
    const names = await page.$$eval('[role="tab"]', l => l.map(t => t.textContent.trim()));
    assert(names.join('|') === 'Console|Notebook', 'tabs ' + names);
    assert(await page.$eval('[role="tablist"]', e => e.closest('.bar') !== null), 'the tablist is in the bar');
    const consistent = () => page.evaluate(() => [...document.querySelectorAll('[role="tab"]')].every(t =>
      (t.getAttribute('aria-selected') === 'true') === !document.getElementById(t.getAttribute('aria-controls')).hidden &&
      (t.getAttribute('aria-selected') === 'true') === (t.tabIndex === 0)));
    const sel = () => page.evaluate(() => document.querySelector('[role="tab"][aria-selected="true"]').id);
    const vis = async () => ({
      nbBar: await shownIn(page, '#nb-toolbar'), bar: await shownIn(page, '.toolbar[aria-label="Console controls"]'),
      pill: await shownIn(page, '#status'), nbPill: await shownIn(page, '#nb-status'),
    });
    assert(await sel() === 'tab-notebook' && await consistent(), 'the notebook is selected');
    let v = await vis();
    assert(v.nbBar && !v.bar && !v.pill && v.nbPill, 'notebook chrome ' + JSON.stringify(v));
    assert(await page.evaluate(() => localStorage.getItem('pharo-wasm.tab')) === 'notebook', 'the tab is remembered');
    await page.focus('#tab-notebook');
    await page.keyboard.press('ArrowLeft');
    assert(await sel() === 'tab-console' && await consistent(), 'ArrowLeft');
    v = await vis();
    assert(!v.nbBar && v.bar && v.pill && !v.nbPill, 'Console chrome ' + JSON.stringify(v));
    await page.keyboard.press('ArrowRight');
    assert(await sel() === 'tab-notebook' && await consistent(), 'ArrowRight');
    await page.keyboard.press('Home');
    assert(await sel() === 'tab-console' && await consistent(), 'Home');
    await page.keyboard.press('End');
    assert(await sel() === 'tab-notebook' && await consistent(), 'End');
    assert(await page.evaluate(() => document.activeElement.id) === 'tab-notebook', 'the focus follows');
    assert(vmWorkers() === 2, 'no more workers: ' + vmWorkers());
  });
  await page.click('#tab-notebook');    // whatever happened above

  await check('Run all on the example notebook stops at its error', async () => {
    await runExample(page);
  });

  await check('rich output: a table, an SVG in currentColor, a progress bar in place, a Form as an image', async () => {
    await page.waitForFunction(() => [...document.querySelectorAll('#nb-cells .nb-rich img')].some(i => i.complete && i.naturalWidth > 0),
                               null, { timeout: 10000 });
    const r = await page.evaluate(() => {
      const rich = [...document.querySelectorAll('#nb-cells .nb-rich')];
      const table = rich.find(e => e.querySelector('table'));
      const svg = document.querySelector('#nb-cells .nb-rich svg');
      const painted = svg && [...svg.querySelectorAll('*')].filter(e => /currentcolor/i.test((e.getAttribute('fill') || '') +
                                                                                           (e.getAttribute('stroke') || '')));
      const prog = [...document.querySelectorAll('#nb-cells > li')].find(li => li.querySelector('progress'));
      const img = [...document.querySelectorAll('#nb-cells .nb-rich img')].find(i => i.naturalWidth > 0);
      return {
        table: !!table && !!table.querySelector('th') && table.querySelectorAll('td').length > 0,
        current: painted ? painted.map(e => { const s = getComputedStyle(e); return [s.color, s.fill, s.stroke]; }) : null,
        progress: prog ? [...prog.querySelectorAll('progress')].map(p => p.getAttribute('value')) : null,
        img: img ? [img.naturalWidth, img.naturalHeight, img.getAttribute('src').slice(0, 22)] : null,
      };
    });
    assert(r.table, 'a table with th and td');
    assert(r.current && r.current.length && r.current.every(([c, f, s]) => f === c || s === c),
           'currentColor follows the text: ' + JSON.stringify(r.current));
    assert(r.progress && r.progress.length === 1 && r.progress[0] !== null, 'one progress bar: ' + JSON.stringify(r.progress));
    assert(r.img && r.img[0] > 0 && r.img[2] === 'data:image/png;base64,', 'a Form as a PNG: ' + JSON.stringify(r.img));
  });

  await check('a Form 0 pixels wide or high shows as an empty image, not as one too large', async () => {
    for (const [code, text] of [['Form extent: 0 @ 0 depth: 32', 'empty image 0x0'],
                                ['Notebook show: (Form extent: 0 @ 5 depth: 1). nil', 'empty image 0x5']]) {
      const i = await nb.eval(code);
      assert(await nb.status(i) === 'ok', code + ': ' + await nb.out(i));
      const out = await nb.out(i);
      assert(out.includes(text) && !/too large/i.test(out), code + ': ' + JSON.stringify(out));
    }
  });

  if (t.shots) {
    await page.mouse.move(0, 0);
    await page.$eval('#nb-cells .nb-rich svg', e => e.scrollIntoView({ block: 'center' }));
    for (const scheme of ['light', 'dark']) {
      await page.emulateMedia({ colorScheme: scheme });
      for (const [w, h] of [[1280, 800], [360, 740], [320, 640]]) {
        await page.setViewportSize({ width: w, height: h });
        await page.waitForTimeout(400);
        await page.screenshot({ path: t.shot(`notebook-${scheme}-${w}.png`) });
      }
    }
    await page.emulateMedia({ colorScheme: 'light' });
    await page.setViewportSize({ width: 1280, height: 800 });
  }

  await check('Shift+Enter, Ctrl+Enter and Alt+Enter', async () => {
    const i = await nb.add('3 + 4');
    const before = await nb.cells();
    await nb.run(i, null, 'Shift+Enter');
    await nb.wait(i);
    assert((await nb.values(i)).join() === '7', 'value ' + await nb.values(i));
    assert(/^\[\d+\]$/.test(await nb.count(i)), 'count ' + await nb.count(i));
    assert(await nb.cells() === before + 1, 'a cell was appended');
    const focus = () => page.evaluate(() => {
      const a = document.activeElement, li = a.closest('#nb-cells > li');
      return { src: a.classList.contains('nb-src'), i: li ? [...li.parentNode.children].indexOf(li) : -1 };
    });
    let f = await focus();
    assert(f.src && f.i === i + 1, 'the focus in the next editor: ' + JSON.stringify(f));
    await nb.run(i + 1, '6 * 7', 'Control+Enter');
    await nb.wait(i + 1);
    assert((await nb.values(i + 1)).join() === '42', 'Ctrl+Enter value');
    f = await focus();
    assert(f.src && f.i === i + 1, 'Ctrl+Enter keeps the focus: ' + JSON.stringify(f));
    await nb.run(i + 1, '50 - 8', 'Alt+Enter');
    await nb.wait(i + 1, /^ok$/);
    f = await focus();
    assert(await nb.cells() === before + 2 && f.src && f.i === i + 2, 'Alt+Enter inserts below: ' + JSON.stringify(f));
  });

  await check('stdout, stderr and the Transcript in the order they were written', async () => {
    const i = await nb.eval("Stdio stdout nextPutAll: 'one'; lf; flush. Stdio stderr nextPutAll: 'two'; lf; flush. " +
                            "Transcript show: 'three'; cr; flush. nil");
    assert(await nb.status(i) === 'ok', 'ok: ' + await nb.out(i));
    const parts = await page.$$eval(`#nb-cells > li:nth-child(${i + 1}) .nb-stream`, l => l.map(e => e.className + ':' + e.textContent.trim()));
    assert(parts.join('|') === 'nb-stream stdout:one|nb-stream stderr:two|nb-stream stdout:three', 'outputs ' + parts.join('|'));
  });

  await check('a VM of its own: the Console does not see the notebook\'s variables, nor it the Console\'s', async () => {
    const a = await nb.eval('nbOnly := 1');
    assert((await nb.values(a)).join() === '1', 'assigned: ' + await nb.out(a));
    const loop = await nb.add('[true] whileTrue');
    await nb.run(loop);
    await nb.wait(loop, /^running$/);
    await page.click('#tab-console');
    assert(/Undeclared/.test(await evalIn(page, 'nbOnly', /\nst> $/)), 'the Console: ' + (await term()).slice(-300));
    await evalIn(page, '3 + 4', /\n7\nst> $/);
    await evalIn(page, 'Smalltalk at: #NbConsoleLate put: 5', /\n5\nst> $/);
    await page.click('#tab-notebook');
    assert(await nb.status(loop) === 'running', 'the cell still runs');
    await page.click('#nb-stop');
    await nb.wait(loop, /^interrupted$/, 5000);
    const b = await nb.eval('Smalltalk at: #NbConsoleLate ifAbsent: [ #none ]');
    assert((await nb.values(b)).join() === '#none', 'the notebook: ' + await nb.out(b));
  });

  await check('Stop: the button, i i and Ctrl+C; Esc in an editor stops nothing', async () => {
    const a = await nb.add('1 to: SmallInteger maxVal do: [ :i | ]');
    await nb.run(a);
    await nb.wait(a, /^running$/);
    await page.click('#nb-stop');
    await nb.wait(a, /^interrupted$/, 5000);
    await nb.src(a).click();
    await nb.run(a);
    await nb.wait(a, /^running$/);
    const before = await term();
    await nb.src(a).press('Escape');
    assert(await page.evaluate(() => document.activeElement.matches('#nb-cells > li')), 'command mode');
    await page.waitForTimeout(500);
    assert(await nb.status(a) === 'running', 'Esc stopped the cell');
    assert(await term() === before && await page.$eval('#status', e => e.dataset.state) === 'waiting', 'Esc reached the Console');
    await page.keyboard.press('i');
    await page.keyboard.press('i');
    await nb.wait(a, /^interrupted$/, 5000);
    await nb.src(a).click();
    await nb.run(a);
    await nb.wait(a, /^running$/);
    await nb.src(a).press('Control+c');
    await nb.wait(a, /^interrupted$/, 5000);
    assert(await term() === before, 'Ctrl+C reached the Console');
    const b = await nb.eval('2 + 2');
    assert((await nb.values(b)).join() === '4', 'the next cell works');
  });

  await check('output streams while the cell runs', async () => {
    const i = await nb.add('1 to: 3 do: [ :i | Transcript show: i printString; cr. (Delay forSeconds: 1) wait ]');
    await nb.run(i);
    await page.waitForFunction(n => {
      const li = document.querySelectorAll('#nb-cells > li')[n];
      return /^1\s/.test(li.querySelector('.nb-out').textContent) &&
        /^(running|sleeping)$/.test(document.getElementById('nb-status').dataset.state);
    }, i, { timeout: 5000 });
    await nb.wait(i, NB_DONE, 10000);
    assert(await nb.status(i) === 'ok', 'ok');
  });

  await check('a syntax error evaluates nothing, and its line is marked', async () => {
    const i = await nb.eval("Transcript show: 'x'; cr; flush.\n#(1 2");
    assert(await nb.status(i) === 'error', 'error status');
    const out = await nb.out(i);
    assert(/Syntax error/.test(out) && /nothing was evaluated/.test(out), 'note: ' + out);
    assert(!(await page.$(`#nb-cells > li:nth-child(${i + 1}) .nb-stream`)), 'no output');
    const mark = await page.$eval(`#nb-cells > li:nth-child(${i + 1})`, e => (e.querySelector('.nb-hl mark') || {}).textContent);
    assert(mark === '#(1 2', 'the line marked: ' + JSON.stringify(mark));
    // the marked line keeps its spans and their colors
    const hl = await page.$eval(`#nb-cells > li:nth-child(${i + 1})`, e => {
      const o = e.querySelector('.nb-hl'), m = o.querySelector('mark'), root = getComputedStyle(document.documentElement);
      const rgb = v => { const x = v.trim(); return 'rgb(' + [1, 3, 5].map(k => parseInt(x.slice(k, k + 2), 16)).join(', ') + ')'; };
      return { text: [...o.children].map(d => d.textContent).join('\n'), src: e.querySelector('.nb-src').value,
               spans: [...m.querySelectorAll('span')].map(s => [s.className, getComputedStyle(s).color,
                                                                rgb(root.getPropertyValue('--' + s.className.replace('char', 'string')))]) };
    });
    assert(hl.text === hl.src && hl.spans.length >= 3 && hl.spans.every(([, c, want]) => c === want) &&
           hl.spans.some(([k]) => k === 'syn-quote') && hl.spans.some(([k]) => k === 'syn-number'),
           'the marked line ' + JSON.stringify(hl));
    const p = await nb.eval('<primitive: 1> 3');
    assert(await nb.status(p) === 'error' && /pragmas are not allowed/.test(await nb.out(p)), 'a pragma: ' + await nb.out(p));
  });

  await check('big output is capped, and the kernel stays responsive', async () => {
    const i = await nb.eval('1 to: 3000 do: [ :k | Transcript show: (String new: 1000 withAll: $x) ]', 120000);
    const r = await page.$eval(`#nb-cells > li:nth-child(${i + 1})`, e => ({
      text: [...e.querySelectorAll('.nb-stream')].reduce((n, s) => n + s.textContent.length, 0),
      note: [...e.querySelectorAll('.nb-note')].map(n => n.textContent).join(' '),
    }));
    assert(r.text <= 1.05 * 1024 * 1024, 'kept ' + r.text + ' characters');
    assert(/omitted/.test(r.note), 'note ' + r.note);
    const t0 = Date.now();
    const j = await nb.eval('1 + 1', 5000);
    assert((await nb.values(j)).join() === '2' && Date.now() - t0 < 5000, 'responsive');
  });

  await check('a flood of shows is capped while the cell runs; ids still update', async () => {
    const i = await nb.eval("Notebook show: 'p0' id: 'p'. 1 to: 3000 do: [ :k | Notebook show: k ]. " +
                            "Notebook show: 'p1' id: 'p'. #done", 120000);
    const r = await page.$eval(`#nb-cells > li:nth-child(${i + 1})`, e => ({
      outs: e.querySelector('.nb-out').children.length,
      rich: [...e.querySelectorAll('.nb-rich')].map(d => d.textContent.trim()),
      note: [...e.querySelectorAll('.nb-note')].map(n => n.textContent).join(' '),
    }));
    assert(r.outs === 1002, r.outs + ' outputs');      // 500, the note, 500, the value
    assert(r.rich.length === 1000 && r.rich[0] === 'p1' && r.rich[1] === '1' && r.rich[999] === '3000',
           'displays ' + JSON.stringify([r.rich.length, r.rich.slice(0, 2), r.rich.slice(-1)]));
    assert(/^\u2026 2,001 outputs omitted \u2026$/.test(r.note), 'note ' + r.note);
    assert((await nb.values(i)).join() === '#done', 'value ' + await nb.values(i));
  });

  await check('markup is sanitized: html:, svg: and markdown: outputs, Markdown cells, imports', async () => {
    const requests = [], dialogs = [];
    const onReq = r => { const u = r.url(); if (!u.startsWith(base) && !/^(data|blob):/.test(u)) requests.push(u); };
    const onDlg = d => {
      if (/^Replace the current notebook/.test(d.message())) d.accept().catch(() => {});
      else { dialogs.push(d.message()); d.dismiss().catch(() => {}); }
    };
    page.on('request', onReq);
    page.on('dialog', onDlg);
    try {
      const html = [
        '<img src=x onerror="window.__pwned=1">',
        '<a href="jav&#x09;ascript:window.__pwned=1">a</a>',
        '<svg><script>window.__pwned=1</script></svg>',
        '<svg><animate attributeName="href" values="javascript:window.__pwned=1"/></svg>',
        '<svg><a href="javascript:window.__pwned=1"><text y="20">a</text></a></svg>',
        '<style>body { display: none }</style>',
        '<iframe srcdoc="<script>parent.__pwned=1</script>"></iframe>',
        '<form><button formaction="javascript:window.__pwned=1">b</button></form>',
        '<div style="background:url(http://example.invalid/x)">bg</div>',
        '<math><mtext><table><mglyph><style><img src=x onerror="window.__pwned=1">',
        '<object data="http://example.invalid/o"></object><embed src="http://example.invalid/e">',
        '<div role="group" aria-label="evil" aria-owns="nb-run-all nb-restart" aria-controls="nb-cells" ' +
          'aria-activedescendant="nb-title" aria-flowto="tab-console">own</div>',
        '<svg><a href="https://example.com/" tabindex="1"><text y="20">t</text></a>' +
          '<rect tabindex="2" width="5" height="5"/></svg>',
        // images through CSS functions other than url(), and escapes
        '<svg><rect width="5" height="5" mask="image-set(\'http://example.invalid/m.png\' 1x)"/>' +
          '<rect width="5" height="5" mask="-webkit-image-set(\'http://example.invalid/w.png\' 1x)"/>' +
          '<rect width="5" height="5" fill="\\75 rl(http://example.invalid/f.png)"/></svg>',
        '<div style="background:image(\'http://example.invalid/i.png\')">i</div>',
        // a base URI for the parser's document, which the CSP would report
        '<base href="http://example.invalid/b/"><a href="rel">rel</a>',
        // HTML inside SVG, which the sanitizer drops (well-formed: the svg:
        // of it is parsed as XML)
        '<svg><foreignObject><div onclick="window.__pwned=1">fo</div></foreignObject>' +
          '<title><img src="x" onerror="window.__pwned=1"/></title></svg>',
      ];
      const svgs = html.filter(s => s.startsWith('<svg'));
      const md = ['[x](javascript:window.__pwned=1)', '![x](http://example.invalid/t.png)', ...html];
      const src = html.map(s => `Notebook show: (Notebook html: ${st(s)}).`).join('\n') + '\n' +
        svgs.map(s => `Notebook show: (Notebook svg: ${st(s)}).`).join('\n') + '\n' +
        md.map(s => `Notebook show: (Notebook markdown: ${st(s)}).`).join('\n');
      const i = await nb.eval(src);
      assert(await nb.status(i) === 'ok', 'cell ok: ' + await nb.out(i));
      await page.click('#nb-end-text');
      const m = (await nb.cells()) - 1;
      await nb.src(m).fill(md.join('\n\n'));
      await nb.src(m).press('Shift+Enter');
      const outs = [...html.map(s => ({ k: 'display', mime: 'text/html', data: s, id: null })),
                    ...svgs.map(s => ({ k: 'display', mime: 'image/svg+xml', data: s.replace('<svg', '<svg xmlns="http://www.w3.org/2000/svg"'), id: null })),
                    ...md.map(s => ({ k: 'display', mime: 'text/markdown', data: s, id: null }))];
      const nbJson = JSON.stringify({ format: 'pharo-notebook', version: 1, meta: { title: 'xss' }, cells: [
        { id: 'x1', type: 'code', source: '1', count: 1, outputs: outs },
        { id: 'x2', type: 'markdown', source: md.join('\n\n') }] });
      const scan = () => page.evaluate(() => {
        const bad = [];
        for (const e of document.querySelectorAll('#panel-notebook *')) {
          for (const a of e.attributes) if (/^on/i.test(a.name)) bad.push(e.localName + '[' + a.name + ']');
          if (/^(script|iframe|foreignobject|animate|set|style|object|embed|math|form|base|x-base)$/i.test(e.localName)) bad.push(e.localName);
          const src = e.getAttribute('src') || (e.localName === 'a' ? '' : e.getAttribute('href')) || '';
          if (/^\s*(javascript|vbscript):|example\.invalid/i.test(src)) bad.push(e.localName + ' ' + src);
          if (/^\s*(javascript|vbscript|data):/i.test(e.getAttribute('href') || '')) bad.push(e.localName + ' href');
          if (Number(e.getAttribute('tabindex')) > 0) bad.push(e.localName + ' tabindex');
          const st = e.getAttribute('style') || '';
          if (/url\(/i.test(st)) bad.push('style ' + st);
          // (markdown links the URLs in the text: links load nothing)
          if (e.closest('.nb-rich, .nb-md')) for (const a of e.attributes)
            if (/example\.invalid|image-set|\\/i.test(a.value) && !(e.localName === 'a' && a.name === 'href'))
              bad.push(e.localName + '[' + a.name + ']=' + a.value);
          // no HTML inside an SVG
          if (e.namespaceURI === 'http://www.w3.org/1999/xhtml' && e.closest('svg')) bad.push('html in svg: ' + e.localName);
          const box = e.closest('.nb-rich, .nb-md');
          if (box) for (const a of e.attributes) {
            if (!/^aria-/.test(a.name)) continue;
            for (const id of a.value.split(/\s+/)) {
              const t = id && document.getElementById(id);
              if (t && t.closest('.nb-rich, .nb-md') !== box) bad.push(a.name + '=' + id);
            }
          }
        }
        return { bad, pwned: window.__pwned, rich: document.querySelectorAll('#nb-cells .nb-rich').length };
      });
      await page.waitForTimeout(300);
      let r = await scan();
      assert(!r.bad.length && r.pwned === undefined, 'outputs and markdown: ' + JSON.stringify(r));
      await accepting(() => page.setInputFiles('#nb-file', { name: 'xss.json', mimeType: 'application/json', buffer: Buffer.from(nbJson) }));
      await page.waitForFunction(() => document.querySelectorAll('#nb-cells > li').length === 2);
      await page.waitForTimeout(300);
      r = await scan();
      assert(r.rich >= outs.length - 2, 'imported outputs rendered: ' + r.rich);
      assert(!r.bad.length && r.pwned === undefined, 'imported: ' + JSON.stringify(r));
      assert(!dialogs.length, 'dialogs: ' + dialogs);
      assert(!requests.length, 'requests: ' + requests);
    } finally {
      page.off('request', onReq);
      page.off('dialog', onDlg);
    }
  });

  await check('Markdown cells render and edit, in time linear in their source', async () => {
    await page.click('#nb-end-text');
    const i = (await nb.cells()) - 1;
    assert(await nb.src(i).evaluate(e => e === document.activeElement), 'a new text cell in edit mode');
    await nb.src(i).fill('## Title\n\nSome *em*, **strong** and `code`.\n\n- one\n- two\n\n```\n3 + 4\n```\n\n| a | b |\n|---|---|\n| 1 | 2 |');
    await nb.src(i).press('Shift+Enter');
    const r = await page.$eval(`#nb-cells > li:nth-child(${i + 1}) .nb-md`, e => ({
      h2: (e.querySelector('h2') || {}).textContent, em: !!e.querySelector('em'), strong: !!e.querySelector('strong'),
      code: !!e.querySelector('p code'), li: e.querySelectorAll('ul li').length, pre: (e.querySelector('pre code') || {}).textContent,
      td: e.querySelectorAll('table td').length, visible: !!e.offsetParent,
    }));
    assert(r.h2 === 'Title' && r.em && r.strong && r.code && r.li === 2 && r.pre === '3 + 4' && r.td === 2 && r.visible,
           'rendered ' + JSON.stringify(r));
    assert(!(await nb.src(i).isVisible()), 'the editor hidden');
    await nb.cell(i).locator('.nb-md').dblclick();
    assert(await nb.src(i).isVisible() && await nb.src(i).evaluate(e => e === document.activeElement), 'a double-click edits');
    await nb.src(i).press('Shift+Enter');
    assert(await nb.cell(i).locator('.nb-md h2').isVisible(), 'rendered again');
    // about 500 KB of input each
    const slow = await page.evaluate(() => {
      const L = window.NotebookLib;
      const inputs = {
        'backtick runs, longest first': Array.from({ length: 1000 }, (_, i) => '`'.repeat(1000 - i)).join('a'),
        'backtick runs, shortest first': Array.from({ length: 1000 }, (_, i) => '`'.repeat(i + 1)).join('a'),
        'unclosed backticks and brackets': '`[a'.repeat(170000),
        'unclosed emphasis': '*a _b ~~c '.repeat(50000),
        'nested brackets': '['.repeat(250000) + ']'.repeat(250000),
      };
      const r = [];
      for (const [k, s] of Object.entries(inputs)) {
        const t = performance.now();
        L.renderMarkdown(s, document, 'perf-');
        const ms = performance.now() - t;
        if (ms > 1000) r.push(k + ': ' + Math.round(ms) + ' ms');
      }
      // short table rows: one cell for the missing ones, not N each
      const N = 3000, t = performance.now();
      const table = L.renderMarkdown('|a'.repeat(N) + '\n' + '|-'.repeat(N) + '\n' + '|\n'.repeat(N), document, 'perf-');
      const tableMs = performance.now() - t, tds = table.querySelectorAll('td');
      if (tableMs > 1000) r.push('short table rows: ' + Math.round(tableMs) + ' ms');
      const ragged = [...L.renderMarkdown('|a|b|c|\n|-|-|-|\n|1|\n|1|2|3|4|', document, 'perf-')
        .querySelectorAll('tbody tr')].map(tr => [...tr.children].map(td => td.textContent + '/' + td.colSpan).join(' '));
      // a closer must be as long as the opener
      const code = [...L.renderMarkdown('`a``b` and ``c`d``', document, 'perf-').querySelectorAll('code')].map(e => e.textContent);
      return { r, code, tds: tds.length, ragged };
    });
    assert(!slow.r.length, 'slow markdown: ' + slow.r.join('; '));
    assert(slow.tds === 6000, 'short table rows: ' + slow.tds + ' cells');
    assert(JSON.stringify(slow.ragged) === JSON.stringify(['1/1 /2', '1/1 2/1 3/1']), 'ragged rows ' + JSON.stringify(slow.ragged));
    assert(JSON.stringify(slow.code) === JSON.stringify(['a``b', 'c`d']), 'code spans ' + JSON.stringify(slow.code));
  });

  // ---- the coloring of the code cells

  // the overlay of cell i: its text, its spans, and its box against the editor's
  const paintOf = i => page.$eval(`#nb-cells > li:nth-child(${i + 1})`, e => {
    const hl = e.querySelector('.nb-hl'), src = e.querySelector('.nb-src'), cs = getComputedStyle(src);
    const a = hl.getBoundingClientRect(), b = src.getBoundingClientRect();
    return { syn: e.querySelector('.nb-editor').classList.contains('syn'), src: src.value, tag: hl.tagName,
             text: hl.firstElementChild ? [...hl.children].map(d => d.textContent).join('\n') : hl.textContent,
             kinds: [...hl.querySelectorAll('span')].map(s => s.className + ':' + s.textContent),
             color: cs.color, caret: cs.caretColor, hlColor: getComputedStyle(hl).color, hidden: hl.getAttribute('aria-hidden'),
             inert: hl.inert, box: [a.left - b.left, a.top - b.top, a.width - b.width, a.height - b.height].map(x => Math.abs(x) < 1),
             heights: [a.height, b.height] };
  });
  const CLEAR = /rgba\(0, 0, 0, 0\)|transparent/;

  await check('code cells are colored, every kind of token; the overlay is inert and lines up with the editor', async () => {
    const src = '"a comment" | t | t := #(1 $a foo #bar: nil) , #[1 2].\n' +
                'Transcript show: \'it\'\'s\'; cr.\n' +
                '[ :x | x > 16r1F ifTrue: [ ^ self ] ] value: 2r1010e2 + 1.5s2 - 3.\n' +
                '#at:put: numArgs + #+ size + #\'with space\' size. super yourself. thisContext. true & false';
    const i = await nb.add(src);
    let r = await paintOf(i);
    assert(r.syn && r.text === r.src && r.tag === 'PRE' && r.hidden === 'true' && r.inert, 'the overlay mirrors the source ' + JSON.stringify(r));
    for (const k of ['syn-comment:"a comment"', 'syn-string:\'it\'\'s\'', 'syn-char:$a', 'syn-number:16r1F', 'syn-number:2r1010e2',
                     'syn-number:1.5s2', 'syn-quote:#(', 'syn-quote:#[', 'syn-quote:#at:put:', 'syn-quote:#+', 'syn-quote:#\'with space\'', 'syn-quote:foo',
                     'syn-constant:nil', 'syn-constant:true', 'syn-special::=', 'syn-special:^', 'syn-special:self',
                     'syn-special:super', 'syn-special:thisContext', 'syn-keyword:show:', 'syn-keyword:ifTrue:',
                     'syn-global:Transcript'])
      assert(r.kinds.includes(k), k + ' in ' + r.kinds.join(' '));
    for (const k of ['paren:.', 'paren:;', 'paren:|', 'paren:[', 'paren:]'])
      assert(r.kinds.some(x => x.startsWith('syn-' + k)), k + ' in ' + r.kinds.join(' '));
    // a block parameter and a temporary stay plain
    assert(!r.kinds.some(x => /:(?::x|x|t)$/.test(x)), 'plain names ' + r.kinds.join(' '));
    // every kind in its own color, which is the token's
    const colors = await page.$eval(`#nb-cells > li:nth-child(${i + 1}) .nb-hl`, (hl, kinds) => {
      const root = getComputedStyle(document.documentElement), out = {};
      for (const k of kinds) {
        const e = hl.querySelector('.syn-' + k);
        const v = root.getPropertyValue('--syn-' + (k === 'char' ? 'string' : k)).trim();
        out[k] = [e && getComputedStyle(e).color, 'rgb(' + [1, 3, 5].map(j => parseInt(v.slice(j, j + 2), 16)).join(', ') + ')'];
      }
      return out;
    }, SYN_KINDS);
    for (const k of SYN_KINDS) assert(colors[k][0] === colors[k][1], k + ' colored ' + JSON.stringify(colors[k]));
    assert(new Set(SYN_KINDS.filter(k => k !== 'char').map(k => colors[k][0])).size === SYN_KINDS.length - 1,
           'a color per kind ' + JSON.stringify(colors));
    assert(CLEAR.test(r.color) && !CLEAR.test(r.caret) && !CLEAR.test(r.hlColor),
           'the editor shows the caret, the overlay the text ' + JSON.stringify(r));
    assert(r.box.every(x => x), 'overlay and editor in the same box ' + JSON.stringify(r.box));
    // lines up where lines wrap, at tabs and at empty lines, at 320 px too
    // (the same height: the same lines; the same text in the same font)
    const wrapped = '\tx := \'' + 'word '.repeat(60) + '\'.\n\n\t\t"' + 'long comment '.repeat(30) + '"\n' +
                    'y := #(' + 'sym '.repeat(50) + ').\n';
    await nb.src(i).fill(wrapped);
    for (const width of [1280, 320]) {
      await page.setViewportSize({ width, height: 800 });
      await page.waitForTimeout(150);
      r = await paintOf(i);
      assert(r.syn && r.text === r.src && r.box.every(x => x) && Math.abs(r.heights[0] - r.heights[1]) < 1,
             'aligned at ' + width + ' px ' + JSON.stringify([r.box, r.heights]));
      const fonts = await page.$eval(`#nb-cells > li:nth-child(${i + 1})`, e => [e.querySelector('.nb-hl'), e.querySelector('.nb-src')]
        .map(x => { const c = getComputedStyle(x); return [c.fontFamily, c.fontSize, c.lineHeight, c.tabSize, c.paddingLeft, c.paddingTop,
                                                          c.whiteSpace, c.overflowWrap, c.letterSpacing].join('|'); }));
      assert(fonts[0] === fonts[1], 'the same font and box ' + JSON.stringify(fonts));
    }
    await page.setViewportSize({ width: 1280, height: 800 });
    await nb.src(i).fill(src);
    // find in page sees the text once (the overlay is inert)
    const found = await page.evaluate(() => {
      const hits = [];
      getSelection().removeAllRanges();
      for (let k = 0; k < 3 && window.find('16r1F ifTrue', false, false, true); k++)
        hits.push(!!(getSelection().anchorNode && getSelection().anchorNode.parentElement &&
                     getSelection().anchorNode.parentElement.closest('.nb-hl')));
      getSelection().removeAllRanges();
      return hits;
    });
    assert(!found.includes(true), 'find skips the overlay ' + JSON.stringify(found));
    // an IME composes in the editor's own (visible) text
    const ime = async type => {
      await nb.src(i).dispatchEvent(type);
      return nb.src(i).evaluate(e => [getComputedStyle(e).color, getComputedStyle(e.parentNode.querySelector('.nb-hl')).visibility]);
    };
    const [during, after] = [await ime('compositionstart'), await ime('compositionend')];
    assert(!CLEAR.test(during[0]) && during[1] === 'hidden' && CLEAR.test(after[0]) && after[1] === 'visible',
           'composition ' + JSON.stringify([during, after]));
  });

  await check('typing repaints the lines that changed only; a text cell is never colored; m and y switch', async () => {
    const i = await nb.add('a := 1.\nb := \'s\'.\nc := 3');
    // a mark on each line's block: those that typing does not touch stay
    const tag = () => page.$eval(`#nb-cells > li:nth-child(${i + 1}) .nb-hl`, hl => [...hl.children].forEach((d, k) => { d.__k = k; }));
    const tags = () => page.$eval(`#nb-cells > li:nth-child(${i + 1}) .nb-hl`, hl => [...hl.children].map(d => d.__k === undefined ? null : d.__k));
    await tag();
    await nb.src(i).press('Control+End');
    await nb.src(i).pressSequentially(' + 4 "open');
    let r = await paintOf(i);
    assert(r.text === r.src && r.kinds[r.kinds.length - 1] === 'syn-comment:"open', 'repainted on input ' + JSON.stringify(r.kinds));
    assert(JSON.stringify(await tags()) === '[0,1,null]', 'only the last line was repainted ' + JSON.stringify(await tags()));
    // a quote typed at the start makes a string of the rest, taken back it is as it was
    const before = r;
    await tag();
    await nb.src(i).press('Control+Home');
    await nb.src(i).press('\'');
    r = await paintOf(i);
    assert(r.text === r.src && r.kinds[0] === 'syn-string:\'a := 1.', 'a quote typed ' + JSON.stringify(r.kinds));
    await nb.src(i).press('Backspace');
    r = await paintOf(i);
    assert(r.text === before.src && JSON.stringify(r.kinds) === JSON.stringify(before.kinds), 'and taken back ' + JSON.stringify(r.kinds));
    // a text cell is plain; back to code it is colored again
    await nb.src(i).press('Escape');
    await page.keyboard.press('m');
    r = await paintOf(i);
    assert(!r.syn && !r.kinds.length && !r.text, 'a text cell is not colored ' + JSON.stringify(r));
    await page.keyboard.press('y');
    r = await paintOf(i);
    assert(r.syn && r.text === r.src && r.kinds.length, 'code again ' + JSON.stringify(r));
    // a new text cell, in edit mode
    await page.click('#nb-end-text');
    const t = (await nb.cells()) - 1;
    await nb.src(t).fill('x := #(1 2). "c" \'s\'');
    r = await paintOf(t);
    assert(!r.syn && !r.kinds.length && !r.text && !CLEAR.test(r.color), 'a new text cell ' + JSON.stringify(r));
    await nb.src(t).press('Shift+Enter');
  });

  await check('a cell of 32 KB is colored and repainted fast; a larger one stays plain', async () => {
    const line = 'x := y at: 1 put: #sym. "c" \'s\' $a.';   // 36 characters a line
    const big = Array.from({ length: Math.floor(32 * 1024 / (line.length + 1)) }, (_, k) => line).join('\n');
    const i = await nb.add();
    let ms = await nb.src(i).evaluate((e, big) => {
      const t = performance.now();
      e.value = big;
      e.dispatchEvent(new Event('input'));
      return performance.now() - t;
    }, big);
    let r = await paintOf(i);
    assert(r.syn && r.text === r.src && r.src.length <= 32 * 1024 && r.kinds.length > 5000, 'colored: ' + r.src.length + ' characters');
    // an edit in the middle: one line repainted, in a few milliseconds
    const edits = await nb.src(i).evaluate(e => {
      const hl = e.parentNode.querySelector('.nb-hl'), first = hl.firstChild, last = hl.lastChild, times = [];
      for (let k = 0; k < 20; k++) {
        const at = e.value.indexOf('\n', e.value.length >> 1);
        const t = performance.now();
        if (k % 2) e.setRangeText('', at - 1, at, 'end'); else e.setRangeText('1', at, at, 'end');
        e.dispatchEvent(new Event('input'));
        times.push(performance.now() - t);
      }
      return { times, same: hl.firstChild === first && hl.lastChild === last, lines: hl.children.length,
               want: e.value.split('\n').length };
    });
    const worst = Math.max(...edits.times), avg = edits.times.reduce((a, b) => a + b, 0) / edits.times.length;
    console.log('# ' + t.name + ': a 32 KB cell painted in ' + Math.round(ms) + ' ms, an edit repainted in ' + avg.toFixed(1) +
                ' ms on average, ' + worst.toFixed(1) + ' ms at most');
    assert(edits.same && edits.lines === edits.want, 'only the line edited was replaced ' + JSON.stringify(edits));
    assert(ms < 1000 && worst < 250, 'slow: ' + Math.round(ms) + ' ms to paint, ' + worst.toFixed(1) + ' ms an edit');
    // a character more: plain, the editor's own text shown
    await nb.src(i).evaluate(e => { e.value += '\n'.repeat(32 * 1024 - e.value.length + 1); e.dispatchEvent(new Event('input')); });
    r = await paintOf(i);
    assert(!r.syn && !r.kinds.length && !r.text && !CLEAR.test(r.color) && r.src.length === 32 * 1024 + 1, 'plain: ' + JSON.stringify(
      { syn: r.syn, kinds: r.kinds.length, text: r.text.length, color: r.color, src: r.src.length }));
    // the marked line of an error shows in a plain cell too
    await nb.src(i).fill('1 +');
    await nb.src(i).evaluate(e => { e.value = '1 +' + ' '.repeat(32 * 1024); e.dispatchEvent(new Event('input')); });
    await nb.run(i);
    await nb.wait(i);
    r = await paintOf(i);
    const mark = await page.$eval(`#nb-cells > li:nth-child(${i + 1}) .nb-hl mark`, m => [m.textContent.slice(0, 3), getComputedStyle(m).color]);
    assert(!r.syn && mark[0] === '1 +' && CLEAR.test(mark[1]), 'a plain cell marked ' + JSON.stringify(mark));
    await nb.src(i).fill('');
  });

  await check('the highlighter: its pieces add up to the source, its brackets are the scanner\'s, in linear time', async () => {
    const r = await page.evaluate(() => {
      const S = window.NotebookSt, L = window.NotebookLib, bad = [];
      let seed = 7;
      const rnd = n => (seed = (seed * 1103515245 + 12345) & 0x7fffffff) % n;
      const A = ['(', ')', '[', ']', '{', '}', '#(', '#[', '"', '\'', '$', '#', ':', ':=', '^', '.', ';', '|', '<', '>', '-', '1',
                 '16r', 'e', 's', '_', 'a', 'B', 'nil', 'self', 'at:', ' ', '\n', '\t', 'é', '𝄞', '!', '\r'];
      const inputs = ['', '"', '\'', '$', '#', '#(', '#[', '$𝄞', '"a\'b"\'c"d\'', '#(#( ] } $) ) )', '#[1 ( ] 2', '\'\'\'\''];
      for (let k = 0; k < 20000; k++) { let s = ''; for (let m = rnd(24); m > 0; m--) s += A[rnd(A.length)]; inputs.push(s); }
      for (const s of inputs) {
        const p = S.highlight(s);
        if (p.map(x => x[1]).join('') !== s) { bad.push('texts of ' + JSON.stringify(s)); continue; }
        if (p.some(x => !x[1] || (x[0] && !/^(comment|string|char|number|quote|constant|special|keyword|paren|global)$/.test(x[0]))))
          bad.push('pieces of ' + JSON.stringify(s) + ': ' + JSON.stringify(p));
        // the brackets that count for the scanner are parens, or quote
        // in a literal array (#( ) #[ ]); a paren is one of them
        const want = new Set(), kind = [];
        S.scan(s, (k, i) => { if (k === 'open' || k === 'close') want.add(i); });
        for (const [k, t] of p) for (let j = 0; j < t.length; j++) kind.push(k);
        for (const i of want) if (kind[i] !== 'paren' && kind[i] !== 'quote') bad.push('bracket ' + i + ' of ' + JSON.stringify(s) + ': ' + kind[i]);
        for (let i = 0; i < s.length; i++)
          if (kind[i] === 'paren' && '()[]{}'.includes(s[i]) && !want.has(i)) bad.push('paren ' + i + ' of ' + JSON.stringify(s));
        // and a line of the overlay is a line of the source
        const lines = L.highlightLines(s, S.highlight);
        if (lines.map(l => l.map(x => x[1]).join('')).join('\n') !== s) bad.push('lines of ' + JSON.stringify(s));
        if (bad.length > 10) break;
      }
      // linear time on 500 K of each
      const slow = [], times = {};
      for (const [k, s] of Object.entries({
        code: 'x := Transcript show: #(1 $a #b) printString; cr. "c" ^ self at: 16r1F put: \'s\'.\n'.repeat(6500),
        comments: '"'.repeat(500000), strings: '\''.repeat(500000), chars: '$'.repeat(500000), hashes: '#'.repeat(500000),
        digits: '1'.repeat(500000), radix: '16r'.repeat(170000), arrays: '#('.repeat(250000), blocks: '['.repeat(500000),
        keywords: 'a:'.repeat(250000), minus: '-1'.repeat(250000),
      })) {
        const t = performance.now();
        S.highlight(s);
        const ms = performance.now() - t;
        times[k] = Math.round(ms);
        if (ms > 1000) slow.push(k + ': ' + Math.round(ms) + ' ms');
      }
      // as DOM: spans for the kinds, text for the rest
      const d = L.highlightDom(document, S.highlight('x := \'a\'.\n^ x'));
      const dom = { text: d.textContent, kids: [...d.childNodes].map(e => e.className || '#text') };
      const lines = JSON.stringify(L.highlightLines('a "b\nc" .\n\n', S.highlight));
      return { bad, slow, times, dom, lines, n: inputs.length };
    });
    console.log('# ' + t.name + ': the highlighter on 500 K (ms) ' + JSON.stringify(r.times));
    assert(!r.bad.length, r.bad.join('\n'));
    assert(!r.slow.length, 'slow: ' + r.slow.join('; '));
    assert(r.dom.text === 'x := \'a\'.\n^ x' && r.dom.kids.join() === '#text,syn-special,#text,syn-string,syn-paren,#text,syn-special,#text',
           'highlightDom ' + JSON.stringify(r.dom));
    assert(r.lines === '[[["","a "],["comment","\\"b"]],[["comment","c\\""],[""," "],["paren","."]],[],[]]', 'highlightLines ' + r.lines);
  });

  await check('the Smalltalk fences of Markdown are colored, lazily and within 32 KB; other languages stay plain', async () => {
    const fences = ['```\n3 + 4\n```', '```st\n#(1) "c"\n```', '```Smalltalk\nself foo: nil\n```', '```pharo\n^ $a\n```',
                    '```python\nif a: "s"\n```', '```js\nlet x = \'s\'\n```', '~~~\nTranscript cr\n~~~'];
    await page.click('#nb-end-text');
    const i = (await nb.cells()) - 1;
    await nb.src(i).fill(fences.join('\n\n'));
    await nb.src(i).press('Shift+Enter');
    const codes = sel => page.$$eval(sel, l => l.map(c => c.querySelectorAll('span').length + ':' + c.textContent));
    const want = ['2:3 + 4', '4:#(1) "c"', '3:self foo: nil', '2:^ $a', '0:if a: "s"', "0:let x = 's'", '1:Transcript cr'];
    let got = await codes(`#nb-cells > li:nth-child(${i + 1}) .nb-md pre code`);
    assert(JSON.stringify(got) === JSON.stringify(want), 'fences ' + JSON.stringify(got));
    // the source of a text cell is not colored while it is edited
    await nb.cell(i).locator('.nb-md').dblclick();
    const r = await paintOf(i);
    assert(!r.syn && !r.kinds.length && !CLEAR.test(r.color), 'its source is plain ' + JSON.stringify(r));
    await nb.src(i).press('Shift+Enter');
    // a markdown: display of the kernel, at once
    const o = await nb.eval(`Notebook markdown: ${st(fences.join('\n\n'))}`);
    got = await codes(`#nb-cells > li:nth-child(${o + 1}) .nb-out pre code`);
    assert(JSON.stringify(got) === JSON.stringify(want), 'a display\'s fences ' + JSON.stringify(got));
    // what renderMarkdown marks, colorCode colors: at most 32 KB of code in all
    const capped = await page.evaluate(() => {
      const L = window.NotebookLib, S = window.NotebookSt;
      const big = '```\n' + 'a := 1.\n'.repeat(2500) + '```\n\n```st\n' + 'b := 2.\n'.repeat(2500) + '```';
      const div = document.createElement('div');
      div.appendChild(L.renderMarkdown(big, document, 't-', { fence: S.FENCE }));
      const before = [...div.querySelectorAll('pre code')].map(c => c.childElementCount > 0);
      L.colorCode(div, S.highlight);
      return [before, [...div.querySelectorAll('pre code')].map(c => c.childElementCount > 0)];
    });
    assert(JSON.stringify(capped) === '[[false,false],[true,false]]', 'the budget ' + JSON.stringify(capped));
  });

  // index.html's colors, as read and composed here: the editor's background,
  // the tint of the selection over it, and the tint of an error's line
  await check('every --syn color is 4.5:1 or more on the editor, under the selection and on an error\'s line, light and dark', async () => {
    const r = await page.evaluate(kinds => {
      const el = document.documentElement, was = el.dataset.theme, out = {};
      const hex = s => [1, 3, 5].map(i => parseInt(s.trim().slice(i, i + 2), 16));
      const lin = c => { c /= 255; return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4; };
      const lum = c => { const [r, g, b] = c.map(lin); return 0.2126 * r + 0.7152 * g + 0.0722 * b; };
      const ratio = (a, b) => { const x = lum(a), y = lum(b); return (Math.max(x, y) + 0.05) / (Math.min(x, y) + 0.05); };
      const over = (top, a, bot) => top.map((c, i) => c * a + bot[i] * (1 - a));
      for (const theme of ['light', 'dark']) {
        el.dataset.theme = theme;
        const cs = getComputedStyle(el), v = n => hex(cs.getPropertyValue(n));
        const bg = v('--term-bg'), grounds = { editor: bg, selection: over(v('--focus'), .18, bg), error: over(v('--term-err'), .18, bg) };
        out[theme] = {};
        for (const k of kinds.concat(['term-fg'])) {
          const name = k === 'term-fg' ? '--term-fg' : '--syn-' + (k === 'char' ? 'string' : k);
          if (!/^#[0-9a-f]{6}$/i.test(cs.getPropertyValue(name).trim())) { out[theme][k] = 'no token ' + name; continue; }
          out[theme][k] = Object.fromEntries(Object.entries(grounds).map(([g, c]) => [g, +ratio(v(name), c).toFixed(2)]));
        }
      }
      if (was) el.dataset.theme = was; else delete el.dataset.theme;
      return out;
    }, SYN_KINDS);
    for (const theme of ['light', 'dark']) {
      const low = Object.entries(r[theme]).filter(([, g]) => typeof g !== 'object' || Object.values(g).some(x => x < 4.5));
      console.log('# ' + t.name + ': contrast, ' + theme + ' (editor/selection/error line) ' +
                  Object.entries(r[theme]).map(([k, g]) => k + ' ' + Object.values(g).join('/')).join(', '));
      assert(!low.length, theme + ': ' + JSON.stringify(low));
    }
    // the selection is that tint: the editor's ::selection of a colored cell
    const sel = await page.evaluate(() => {
      const e = document.querySelector('#nb-cells .nb-editor.syn .nb-src');
      return e ? getComputedStyle(e, '::selection').backgroundColor : null;
    });
    assert(sel && !CLEAR.test(sel), 'the selection is tinted: ' + sel);
  });

  // ten uses of a group of ten uses of ...: 10^5 elements from 1.2 KB
  await check('nested <use> in markup is bounded', async () => {
    const r = await page.evaluate(async () => {
      const L = window.NotebookLib;
      const bomb = lv => {
        let s = '<defs><g id="l0"><rect width="1" height="1"/></g>';
        for (let k = 1; k <= lv; k++) s += `<g id="l${k}">` + `<use href="#l${k - 1}"/>`.repeat(10) + '</g>';
        return s + `</defs><use href="#l${lv}"/>`;
      };
      const sprite = '<defs><symbol id="s"><rect width="2" height="2"/><circle r="1"/></symbol>' +
        '<g id="t"><use href="#s"/><use href="#s" x="3"/></g></defs>' +
        Array.from({ length: 100 }, (_, i) => `<use href="#t" y="${i * 3}"/>`).join('') +
        '<g id="c"><use href="#c"/></g>';
      const doc = s => '<svg xmlns="http://www.w3.org/2000/svg">' + s + '</svg>';
      const out = {};
      const cases = { svg: [doc(bomb(5)), 'svg'], html: ['<p>a</p><svg>' + bomb(5) + '</svg>', 'html'], sprite: [doc(sprite), 'svg'] };
      for (const [k, [src, kind]] of Object.entries(cases)) {
        const box = document.createElement('div');
        box.className = 'nb-rich';
        document.body.appendChild(box);
        const t = performance.now();
        box.appendChild(L.sanitizeMarkup(src, kind, 'use-' + k + '-'));
        box.getBoundingClientRect();
        await new Promise(res => requestAnimationFrame(() => requestAnimationFrame(res)));
        // the elements rendered, each use counting what it copies
        const inst = (el, d) => {
          if (d > 32) return Infinity;
          let n = 1;
          const t = el.localName === 'use' && document.getElementById((el.getAttribute('href') || '').slice(1));
          if (t) n += inst(t, d + 1);
          for (const c of el.children) n += inst(c, d);
          return n;
        };
        out[k] = { ms: Math.round(performance.now() - t), uses: box.querySelectorAll('use').length,
                   top: box.querySelectorAll('svg > use').length, rendered: inst(box, 0) };
        box.remove();
      }
      return out;
    });
    for (const k of ['svg', 'html']) {
      assert(r[k].ms < 1000, k + ' bomb rendered in ' + r[k].ms + ' ms');
      assert(r[k].rendered < 5000, k + ' bomb kept ' + JSON.stringify(r[k]));
    }
    // a sprite well under the budget is kept whole; a cycle is dropped
    assert(r.sprite.uses === 102 && r.sprite.top === 100, 'sprite ' + JSON.stringify(r.sprite));
  });

  // twenty 800x800 rects sharing five blurs and dilations: minutes a paint
  await check('filters in markup are bounded', async () => {
    const r = await page.evaluate(() => {
      const L = window.NotebookLib;
      const doc = s => '<svg xmlns="http://www.w3.org/2000/svg" width="800" height="800">' + s + '</svg>';
      const heavy = '<filter id="f">' + '<feGaussianBlur stdDeviation="30"/><feMorphology operator="dilate" radius="40"/>'.repeat(5) + '</filter>';
      const shadow = '<filter id="s"><feGaussianBlur in="SourceAlpha" stdDeviation="3"/><feOffset dx="2" dy="2"/>' +
        '<feMerge><feMergeNode/><feMergeNode in="SourceGraphic"/></feMerge></filter>';
      const rects = (n, f) => `<rect width="800" height="800" filter="url(#${f})"/>`.repeat(n);
      const cases = {
        heavy: [doc(heavy + rects(20, 'f')), 'svg'],
        shadows: [doc(shadow + rects(20, 's')), 'svg'],
        uses: [doc(shadow + '<defs><g id="g"><rect width="9" height="9" filter="url(#s)"/></g></defs>' + '<use href="#g"/>'.repeat(20)), 'svg'],
        mask: [doc(shadow + '<mask id="m"><rect width="9" height="9" fill="white" filter="url(#s)"/></mask>' +
                   '<rect width="9" height="9" mask="url(#m)"/>'), 'svg'],
        css: ['<div style="width:800px;height:800px;filter:' + 'blur(30px) '.repeat(40) + '">x</div>' +
              '<p style="color:red;text-shadow:' + Array(40).fill('0 0 9px red').join(',') + '">t</p>' +
              '<svg width="800" height="800"><rect width="800" height="800" filter="' + 'blur(9px) '.repeat(40) + '"/></svg>' +
              '<p style="filter:blur(1px)">kept</p>', 'html'],
      };
      const out = {};
      for (const [k, [src, kind]] of Object.entries(cases)) {
        const box = document.createElement('div');
        box.appendChild(L.sanitizeMarkup(src, kind, 'flt-' + k + '-'));
        out[k] = { filtered: box.querySelectorAll('[filter]').length, styles: [...box.querySelectorAll('[style]')].map(e => e.getAttribute('style')) };
      }
      return out;
    });
    const J = x => JSON.stringify(x);
    assert(r.heavy.filtered === 0, 'heavy ' + J(r.heavy));
    // a drop shadow weighs 7: nine of them fit
    assert(r.shadows.filtered === 9, 'shadows ' + J(r.shadows));
    assert(r.uses.filtered === 0, 'one filtered element, copied 20 times ' + J(r.uses));
    assert(r.mask.filtered === 0, 'in a mask ' + J(r.mask));
    assert(r.css.filtered === 0 && J(r.css.styles) === J(['width:800px; height:800px', 'color:red', 'filter:blur(1px)']), 'css ' + J(r.css));
  });

  // the browsers parse unclosed elements in time quadratic in their depth,
  // and which ones stay open depends on their context: the markup reaches
  // the parser only as notebook-lib.js's tidyHtml writes it again
  await check('deeply nested HTML is refused before it is parsed, whatever nests it; shallow HTML parses fast; <base> is dropped', async () => {
    const r = await page.evaluate(() => {
      const L = window.NotebookLib;
      const render = src => {
        const t = performance.now();
        try {
          const box = document.createElement('div');
          box.appendChild(L.sanitizeMarkup(src, 'html', 'deep-'));
          return { ms: performance.now() - t, n: box.querySelectorAll('*').length };
        } catch (e) { return { ms: performance.now() - t, error: e.message }; }
      };
      let formatting = '<p>';
      for (let i = 0; i < 3000; i++) formatting += '<b id=f' + i + '>';
      const deep = {
        div: '<div>'.repeat(100000), rt: '<rt>'.repeat(100000), 'li dd': '<li><dd>'.repeat(50000),
        'div/': '<div/>'.repeat(100000), 'div /x': '<div></x>'.repeat(100000), 'div title': '<div title="</div>">'.repeat(50000),
        'div comment': '<div><!--</div>-->'.repeat(50000), 'svg area': '<svg>' + '<area>'.repeat(100000),
        'span hr': '<span>'.repeat(4000) + '<hr>'.repeat(500000), formatting: formatting + '<div>x</div>'.repeat(100000),
      };
      const out = {};
      for (const [k, src] of Object.entries(deep)) out[k] = render(src);
      // shallow, but which the parsers (Chromium's) took seconds over: what
      // rebuild drops with its content (<option>, <fieldset>, <canvas>) is
      // left out before, a <bdi> and dir are not what the parser sees
      const flat = {
        'option br': '<option>' + 't<br>'.repeat(40000), 'option x': '<option>' + '<x>t</x>'.repeat(40000),
        'fieldset br': '<fieldset>' + 't<br>'.repeat(40000), 'option a table': '<option><a id=qK><table>t'.repeat(12000),
        'canvas': '<canvas>' + 't<!---->'.repeat(40000), 'bdi': '<bdi>' + '<span></span>'.repeat(20000),
        'dir auto': '<div dir=auto>' + '<span></span>'.repeat(20000), 'dir auto spans': '<p>' + '<span dir=auto>t</span>'.repeat(20000),
        'th em': '<th><em id=q></th><td>t'.repeat(3000),
      };
      const shallow = {};
      for (const [k, src] of Object.entries(flat)) shallow[k] = render(src);
      return { deep: out, shallow, fair: render('<div>'.repeat(4000) + 'x'),
               flat: render('<p>a<li>b<td>c'.repeat(20000)), base: render('<base href="http://example.invalid/"><p>b</p>'),
               baseURI: document.baseURI === location.href };
    });
    const J = x => JSON.stringify(x);
    for (const [k, d] of Object.entries(r.deep))
      assert(/nests more than 4096|too large for how deeply it nests/.test(d.error || '') && d.ms < 500, k + ' ' + J(d));
    assert(/nests more than 4096/.test(r.deep.rt.error) && /nests more than 4096/.test(r.deep['li dd'].error), 'rt, li dd ' + J(r.deep));
    assert(!r.fair.error && r.fair.n === 65 && r.fair.ms < 2000, 'depth 4000, of which 65 are shown ' + J(r.fair));
    for (const [k, d] of Object.entries(r.shallow)) assert(!d.error && d.ms < 1500, k + ' ' + J(d));
    assert(r.shallow.bdi.n === 20002 && r.shallow['dir auto'].n === 20001 && r.shallow['option br'].n === 0, 'kept ' + J(r.shallow));
    assert(!r.flat.error && r.flat.n >= 20000, 'unclosed <p>, <li> and <td> do not nest ' + J(r.flat));
    assert(!r.base.error && r.base.n === 1 && r.baseURI, '<base> ' + J(r));
    for (const [tag, n] of [['<div>', 100000], ['<rt>', 100000], ['<li><dd>', 50000]]) {
      const t0 = Date.now();
      const i = await nb.eval(`Notebook show: (Notebook html: (String streamContents: [ :s | ${n} timesRepeat: [ s nextPutAll: '${tag}' ] ])). 1`);
      assert(await nb.status(i) === 'ok', 'cell ' + await nb.out(i));
      const note = await nb.cell(i).locator('.nb-note').allTextContents();
      assert(note.some(t => /Could not show the text\/html output: the HTML nests more than 4096/.test(t)), tag + ' note ' + J(note));
      assert(Date.now() - t0 < 10000, tag + ' in ' + (Date.now() - t0) + ' ms');
    }
    // an import of the same, whose outputs render as it loads (with a
    // Markdown cell, as the import of the sanitizer's check, for the checks after)
    const outs = ['<rt>'.repeat(100000), '<li><dd>'.repeat(50000), '<div/>'.repeat(100000)]
      .map(data => ({ k: 'display', mime: 'text/html', data, id: null }));
    const text = JSON.stringify({ format: 'pharo-notebook', version: 1, meta: { title: 'deep' }, cells: [
      { id: 'd1', type: 'code', source: '1', count: 1, outputs: outs }, { id: 'd2', type: 'markdown', source: '# Deep' }] });
    const t0 = Date.now();
    await accepting(() => page.setInputFiles('#nb-file', { name: 'deep.json', mimeType: 'application/json', buffer: Buffer.from(text) }));
    await page.waitForFunction(() => document.querySelectorAll('#nb-cells > li').length === 2);
    await page.evaluate(() => document.querySelectorAll('#nb-cells .nb-hidden-out').forEach(b => { if (!b.hidden) b.click(); }));
    const notes = () => page.$$eval('#nb-cells .nb-note', l => l.map(e => e.textContent).filter(t => /nests more than 4096/.test(t)));
    await page.waitForFunction(() => [...document.querySelectorAll('#nb-cells .nb-note')]
      .filter(e => /nests more than 4096/.test(e.textContent)).length === 3, null, { timeout: 5000 })
      .catch(async () => { throw new Error('notes ' + J(await notes())); });
    const ms = Date.now() - t0;
    assert(ms < 5000, 'the import took ' + ms + ' ms');
  });

  // what the page shows of the markup that tidyHtml writes, as of the
  // markup itself parsed: the elements, their attributes and text, in their places
  await check('tidied HTML shows as the markup did: paragraphs, lists, tables, inline elements, SVG, MathML, entities', async () => {
    const r = await page.evaluate(() => {
      const L = window.NotebookLib;
      const parse = s => new DOMParser().parseFromString('<!doctype html><html><head></head><body>' + s, 'text/html').body;
      // (in a document of its own: the page's would log the errors of the SVG
      // attributes of the corpus to the console)
      const doc = document.implementation.createHTMLDocument('');
      const shown = frag => { const d = doc.createElement('div'); d.appendChild(frag); return d.innerHTML; };
      const corpus = [
        '<p>a<p>b', '<b><i>x</b>y</i>', '<table><tr><td>1<td>2<tr><td>3</table>', '<ul><li>a<li>b</ul>',
        '<a href=x title=\'a"b\' c>q</a>', '<div/>x', '<svg><rect/><g><path d="M0"/></g></svg>z', '<svg><div>x</div></svg>',
        'a<!-- c -->b', '&am<!---->p;', '<style>x</style>y<script>if(a<b)</script>z', '<td>c', 'x</p>y', '<br></br>',
        '<math><mi><b>x</b></mi></math>', '<p><div>x</div>', '<select><option>a<option>b</select>', '<h1>a<h2>b',
        '<dl><dt>a<dd>b<dt>c</dl>', '<svg><![CDATA[a<b&c]]></svg>', '<img src=x alt="<b>">', '<ruby>a<rt>b<rt>c</ruby>',
        '<div title="</div>">x', '<a><a>x', '<textarea><b></textarea>t', '<div></x>x', '<svg><title><b>t</b></title></svg>',
        '<table><caption>c<tr><td>x</td></tr></table>', '<table><div>f</div><tr><td>1</table>', '<p>&amp; &lt; &notin; &not x &copy</p>',
        '<b><div>x</div>y</b>z', '<div><b></div>c', '<table><td>a<table><td>b</table>c</table>', '<ol><li>1<ul><li>a<li>b</ul><li>2</ol>',
        '<p><table><tr><td>x</td></tr></table>', '<form><form>x</form>y</form>', '<button><button>x', '<nobr>a<nobr>b',
        '<u><s><em>x</u>y</s>z</em>', '<pre>\nx</pre>', '<span><div>x</span>y</div>', '<table><tr><th>h<td>d</table>',
        '<svg viewBox="0 0 10 10"><foreignObject><div>x</div></foreignObject><text>t</text></svg>', '<a href="#x"><div>block</div></a>',
        '<p>x<ul><li>y</ul>z', '<b>a<table><tr><td>b</td></tr></table>c</b>', '<object><b>x</object>y',
        '<math><mtext><div>x</div></mtext></math>', '<table><colgroup><col span=2><col></colgroup><tr><td>1</table>',
        'text & more', '<p>unterminated', '<div', '<!doctype html><p>x', '</3>z', '<a><b><a>x</a></b></a>',
        '<ul><li><p>a<li><p>b</ul>', '<ruby>k<rp>(</rp><rt>kan</rt><rp>)</rp></ruby>', '<p>a<br>b<hr>c', '<table>x<tr><td>1</table>',
        '<select><optgroup label=a><option>1<optgroup label=b><option>2</select>', '<p><b>1<i>2</p>3</i>4', '<div><span></div>x</span>',
        '<image src=x>', '<svg><image href=x /><circle r=1/></svg>', '<math><annotation-xml encoding="text/html"><div>x</div></annotation-xml></math>',
        '<table><tr><td><p>a<td>b</table>', '<body bgcolor=red>x</body>y', '<sub>1<sup>2</sub>3</sup>', '<address><p>a</address>b',
        '<h3>t</h4>x', '<p></p></p>', '<table></table></table>x', '<div><table><tr><td>a</div>b</td></tr></table>',
        '<table border=1><thead><tr><th>n</th><th>name</th></tr></thead><tr><td>1</td><td>one</td></tr></table>',
        // what rebuild drops with its content, unwraps, or does not show
        '<option>a<br>b</option>c', '<fieldset><legend>l</legend>x<b>y</fieldset>z', '<option><b>x</option>y',
        '<p>a<form>b<input value=1>c</form>d', '<select><option>a<option>b</select>c', '<canvas>t<!---->u</canvas>v',
        '<bdi>a<b>b</b></bdi>c', '<p dir=rtl>x<bdi dir=ltr>y</bdi><span dir=auto>z</span>', '<foo bar=1>x<div>y</div></foo>z',
        '<listing>a<li>b</listing>c', '<menu><li>a<li>b</menu>', '<details name=a open onclick=x><summary>s</summary>d</details>',
        '<svg><foreignObject><b>x</b></foreignObject><desc>d<b>e</b></desc><animate/><rect onclick=f width=2 viewBox="0 0 1 1"/></svg>',
        '<math><mi>x</mi></math>y', '<div>'.repeat(70) + 'deep<p>er</p>', '<b>'.repeat(70) + 'deep', 'a&amp<option>x</option>;b',
        '<table><tr><td><select><option>1</select>x</td></tr></table>', '<p>1<object>2<p>3</object>4', '<ul><li>a<dialog>b</ul>c',
      ];
      return corpus.map(s => { let t; try { t = L.tidyHtml(s); } catch (e) { return { s, error: e.message }; }
                                const a = shown(L.rebuildHtml(parse(s), 'fid-', 0, doc)), b = shown(L.sanitizeMarkup(s, 'html', 'fid-', doc));
                                return a === b ? null : { s, t, a, b }; }).filter(x => x);
    });
    assert(!r.length, r.length + ' differ: ' + JSON.stringify(r).slice(0, 2000));
  });

  await check('command mode keys; Tab order within a cell', async () => {
    const a = await nb.add('10 + 1');
    await nb.add('10 + 2');
    const focused = () => page.evaluate(() => {
      const a = document.activeElement;
      return a.matches('#nb-cells > li') ? [...a.parentNode.children].indexOf(a) : -1;
    });
    const n0 = await nb.cells();
    await nb.src(a).press('Escape');
    assert(await focused() === a, 'Esc selects the cell');
    await page.keyboard.press('j');
    assert(await focused() === a + 1, 'j');
    await page.keyboard.press('k');
    assert(await focused() === a, 'k');
    await page.keyboard.press('b');
    assert(await nb.cells() === n0 + 1 && await focused() === a + 1, 'b inserts below');
    await page.keyboard.press('d');
    await page.keyboard.press('d');
    assert(await nb.cells() === n0, 'd d deletes');
    assert(await page.isVisible('#nb-toast') && /Undo/.test(await page.textContent('#nb-toast')), 'the Undo toast');
    await page.keyboard.press('z');
    assert(await nb.cells() === n0 + 1, 'z restores');
    await page.keyboard.press('d');
    await page.keyboard.press('d');
    assert(await focused() === a + 1, 'the next cell is selected: ' + await focused());
    await page.keyboard.press('m');
    assert(await nb.cell(a + 1).getAttribute('data-type') === 'markdown', 'm');
    await page.keyboard.press('y');
    assert(await nb.cell(a + 1).getAttribute('data-type') === 'code', 'y');
    assert(await nb.src(a + 1).inputValue() === '10 + 2', 'the source kept');
    await page.keyboard.press('Alt+ArrowUp');
    assert(await nb.src(a).inputValue() === '10 + 2' && await focused() === a, 'Alt+Up moves the cell');
    await page.keyboard.press('Alt+ArrowDown');
    assert(await nb.src(a + 1).inputValue() === '10 + 2', 'Alt+Down moves it back');
    await page.keyboard.press('Shift+Enter');
    await nb.wait(a + 1);
    assert((await nb.values(a + 1)).join() === '12', 'Shift+Enter in command mode runs');
    // Tab reaches the run button, the editor and the tools of a cell
    await nb.cell(a).focus();
    const seen = [];
    for (let k = 0; k < 8 && !/source/.test(seen[seen.length - 1]); k++) {   // (Tab in an editor indents)
      await page.keyboard.press('Tab');
      seen.push(await page.evaluate(() => document.activeElement.getAttribute('aria-label') || document.activeElement.className));
    }
    assert(seen.some(s => /^Run cell/.test(s)) && seen.some(s => /source \(Smalltalk\)/.test(s)) &&
           seen.some(s => /^Move cell .* down/.test(s)) && seen.some(s => /^Delete cell/.test(s)), 'tab order ' + seen.join(', '));
    // Tab inserts a tab, Shift+Tab takes it away
    await page.keyboard.press('Home');
    await page.keyboard.press('Tab');
    assert(await nb.src(a).inputValue() === '\t10 + 1', 'Tab indents: ' + JSON.stringify(await nb.src(a).inputValue()));
    await page.keyboard.press('Shift+Tab');
    assert(await nb.src(a).inputValue() === '10 + 1', 'Shift+Tab: ' + JSON.stringify(await nb.src(a).inputValue()));
  });

  await check('Upload from More, then a file-in in a cell; a drop on the tab uploads too', async () => {
    const [fc] = await Promise.all([page.waitForEvent('filechooser'), nb.menu('upload')]);
    await fc.setFiles({ name: 'nbup.st', mimeType: 'text/plain', buffer: Buffer.from('Smalltalk at: #NbUpVal put: 99!\n') });
    await page.waitForFunction(() => /nbup\.st/.test(document.getElementById('nb-toast-text').textContent));
    const i = await nb.eval("'nbup.st' asFileReference fileIn. Smalltalk at: #NbUpVal");
    assert((await nb.values(i)).join() === '99', 'value ' + await nb.out(i));
    // a drop on the tab: the kernel gets the file, the Console opens nothing
    const before = await term(), said = await page.textContent('#notice-text');
    const d = await dropOn(page, '#panel-notebook', [{ name: 'nbdrop.st', data: 'Smalltalk at: #NbDropVal put: 98!\n' }]);
    assert(!d.shown && d.took && d.hidden, 'the drop ' + JSON.stringify(d));
    await page.waitForFunction(() => /nbdrop\.st/.test(document.getElementById('nb-toast-text').textContent));
    const j = await nb.eval("'nbdrop.st' asFileReference fileIn. Smalltalk at: #NbDropVal");
    assert((await nb.values(j)).join() === '98', 'value ' + await nb.out(j));
    assert(await term() === before && await page.textContent('#notice-text') === said,
           'the Console: ' + (await term()).slice(before.length) + ' / ' + await page.textContent('#notice-text'));
  });

  await check('the theme of More re-themes the notebook without a restart; Settings only where git has HTTP', async () => {
    await nb.eval('nbTheme := 5');
    const inits = await notebookInits();
    const bg = () => page.$eval('.nb-editor', e => getComputedStyle(e).backgroundColor);
    const seen = [];
    for (let k = 0; k < 3; k++) {
      await nb.menu('theme');
      seen.push([await page.evaluate(() => document.documentElement.dataset.theme || 'auto'), await bg()]);
    }
    assert(JSON.stringify(seen.map(s => s[0])) === '["light","dark","auto"]', 'themes ' + JSON.stringify(seen));
    assert(seen[0][1] !== seen[1][1], 'the editors change color: ' + JSON.stringify(seen));
    assert(await page.textContent('#theme-label') === 'System', 'the Console\'s button follows: ' + await page.textContent('#theme-label'));
    const i = await nb.eval('nbTheme');
    assert((await nb.values(i)).join() === '5' && await notebookInits() === inits, 'the kernel was restarted: ' + await nb.out(i));
    const settings = await page.$eval('#nb-menu [data-act="settings"]', e => !e.hidden);
    if (!manifest.gitHttp) assert(!settings, 'Settings shown by a build without git over HTTP');
  });

  await check('Settings of a build with git over HTTP give the proxy to the kernel, without a restart', async () => {
    // a manifest that says so, as for page.spec.mjs's Settings, on any build
    const context = await t.browser.newContext();
    try {
      await context.exposeFunction('__nbCsp', s => { csp.push(s); });
      await context.addInitScript(INIT);
      const body = JSON.stringify(Object.assign({}, manifest, { git: true, gitHttp: true }));
      await context.route(url => url.pathname.endsWith('/manifest.json'), route => route.fulfill({ contentType: 'application/json', body }));
      const p = await context.newPage();
      t.watch(p);
      listen(p);
      const n = nbOf(p);
      await p.goto(base + 'index.html');
      await started(p);
      await p.click('#tab-notebook');
      await n.waitState(/^ready$/);
      await n.eval('nbProxy := 6');
      await n.menu('settings');
      assert(await p.$eval('#settings-dialog', d => d.open), 'the dialog opens');
      await p.fill('#git-proxy', 'http://127.0.0.1:9');
      await p.press('#git-proxy', 'Enter');
      const sent = await posts(p);
      const nbWorker = sent.find(m => m.type === 'init' && m.mode === 'notebook').worker;
      assert(sent.some(m => m.worker === nbWorker && m.type === 'gitProxy' && m.gitProxy === 'http://127.0.0.1:9/'),
             'the kernel got it: ' + JSON.stringify(sent));
      const i = await n.eval('nbProxy');
      assert((await n.values(i)).join() === '6' && await notebookInits(p) === 1, 'the kernel was restarted');
      await p.close();
    } finally { await context.close(); }
  });

  for (const width of [390, 360, 320]) await check(`no horizontal scroll at ${width} px, 16 px gutters`, async () => {
    await page.setViewportSize({ width, height: 740 });
    try {
      const i = await nb.eval('String new: 300 withAll: $x');
      await nb.cell(i).scrollIntoViewIfNeeded();
      const o = await page.evaluate(() => {
        const d = document.documentElement;
        const wide = [...document.querySelectorAll('body *')].filter(e => {
          const r = e.getBoundingClientRect();
          return r.width && (r.right > d.clientWidth + 0.5 || r.left < -0.5) && getComputedStyle(e).visibility !== 'hidden';
        }).map(e => e.tagName + (e.id ? '#' + e.id : '') + (e.className && typeof e.className === 'string' ? '.' + e.className : ''));
        return { sw: d.scrollWidth, cw: d.clientWidth, bw: document.body.scrollWidth, wide: wide.slice(0, 5) };
      });
      assert(o.sw <= o.cw && o.bw <= o.cw && !o.wide.length, JSON.stringify(o));
      const gutter = await page.$eval('#panel-notebook', e => [e.getBoundingClientRect().left,
                                                               document.documentElement.clientWidth - e.getBoundingClientRect().right]);
      assert(gutter.every(g => g >= 15.5 && g <= 16.5), '16 px gutters, got ' + gutter);
      const bar = await page.$eval('.bar', e => [e.scrollWidth, e.clientWidth]);
      assert(bar[0] <= bar[1], 'the bar fits: ' + bar);
      const v = await page.$eval(`#nb-cells > li:nth-child(${i + 1}) .nb-value`, e => [e.scrollWidth, e.clientWidth]);
      assert(v[0] > v[1], 'the long value scrolls in its own box: ' + v);
    } finally { await page.setViewportSize({ width: 1280, height: 800 }); }
  });

  await check('Stop in a primitive that never yields restarts the kernel after the 3 s watchdog', async () => {
    const before = await nb.eval('preKill := 1');
    const i = await nb.add(LONG_PRIMITIVE);
    await nb.run(i);
    await nb.wait(i, /^running$/);
    await page.waitForTimeout(500);
    const inits = await notebookInits(), t0 = Date.now();
    await page.click('#nb-stop');
    await nb.wait(i, NB_DONE, 30000);
    const ms = Date.now() - t0;
    console.log('  # replaced in ' + ms + ' ms');
    assert(await nb.status(i) === 'error', 'status ' + await nb.status(i));
    assert(ms >= 2900, 'not before the 3 s watchdog: ' + ms);
    assert(/did not respond/.test(await nb.out(i)), 'note: ' + await nb.out(i));
    assert(await nb.cell(before).getAttribute('data-stale') === 'true', 'earlier cells stale');
    const j = await nb.eval('5 + 5', 90000);
    assert((await nb.values(j)).join() === '10' && await nb.count(j) === '[1]', '5 + 5: ' + await nb.out(j) + ' ' + await nb.count(j));
    assert(await notebookInits() === inits + 1, 'one respawn: ' + (await notebookInits() - inits));
  });

  // the client last heard "sleeping": the slice that woke up is in the
  // primitive, and reports nothing
  await check('Stop after a Delay, in a primitive that never yields, restarts the kernel', async () => {
    const i = await nb.add('(Delay forSeconds: 1) wait. ' + LONG_PRIMITIVE);
    await nb.run(i);
    await nb.waitState(/^sleeping$/, 10000);
    await page.waitForTimeout(1500);
    const t0 = Date.now();
    await page.click('#nb-stop');
    await nb.wait(i, NB_DONE, 30000);
    const ms = Date.now() - t0;
    assert(ms >= 2900, 'not before the 3 s watchdog: ' + ms);
    assert(/did not respond/.test(await nb.out(i)), 'note: ' + await nb.out(i));
    const j = await nb.eval('5 + 6', 90000);
    assert((await nb.values(j)).join() === '11', '5 + 6');
  });

  // the watcher cannot preempt it, and the worker still ticks: the
  // watchdog's deadline is hard
  await check('Stop of a valueUnpreemptively loop restarts the kernel within 10.5 s', async () => {
    const before = await nb.eval('preH1 := 1');
    const i = await nb.add('[ [ true ] whileTrue ] valueUnpreemptively');
    await nb.run(i);
    await nb.wait(i, /^running$/);
    await page.waitForTimeout(1500);
    const t0 = Date.now();
    await page.click('#nb-stop');
    await nb.wait(i, NB_DONE, 30000);
    const ms = Date.now() - t0;
    console.log('  # replaced in ' + ms + ' ms');
    assert(ms >= 2900 && ms < 10500, 'replaced in ' + ms + ' ms');
    assert(/did not respond/.test(await nb.out(i)), 'note: ' + await nb.out(i));
    assert(await nb.cell(before).getAttribute('data-stale') === 'true', 'earlier cells stale');
    await nb.waitState(/^(ready|off)$/);
  });

  await check('Restart resets the counter and the variables', async () => {
    const a = await nb.eval('beforeRestart := 1');
    await page.click('#nb-restart');
    await nb.waitState(/^ready$/);
    assert(await nb.cell(a).getAttribute('data-stale') === 'true', 'the old output stale');
    const i = await nb.eval('beforeRestart');
    assert(await nb.count(i) === '[1]', 'count ' + await nb.count(i));
    assert(await nb.status(i) === 'error' && /Undeclared/.test(await nb.out(i)), 'gone: ' + await nb.out(i));
  });

  await check('export and import .st and .json; the chunk format keeps what reads as a marker', async () => {
    const cells = () => page.$$eval('#nb-cells > li', l => l.map(e => [e.dataset.type, e.querySelector('.nb-src').value]));
    const before = await cells();
    for (const [act, ext] of [['export-st', '.st'], ['export-json', '.json']]) {
      const [dl] = await Promise.all([page.waitForEvent('download'), nb.menu(act)]);
      assert(dl.suggestedFilename().endsWith(ext), 'file name ' + dl.suggestedFilename());
      const text = fs.readFileSync(await dl.path(), 'utf8');
      if (ext === '.st') assert(/^"Pharo notebook: /.test(text) && /^"%%"$/m.test(text) && /^"%% \[markdown\]$/m.test(text), 'the markers');
      else assert(JSON.parse(text).format === 'pharo-notebook', 'the json format');
      await page.click('#nb-end-code');            // changed, then replaced by the import
      await accepting(() => page.setInputFiles('#nb-file', { name: 'nb' + ext, mimeType: 'text/plain', buffer: Buffer.from(text) }));
      await page.waitForFunction(n => document.querySelectorAll('#nb-cells > li').length === n, before.length);
      const after = await cells();
      assert(JSON.stringify(after) === JSON.stringify(before), ext + ' round trip:\n' + JSON.stringify(before) + '\n' + JSON.stringify(after));
    }
    const r = await page.evaluate(() => {
      const S = window.NotebookSt, L = window.NotebookLib;
      // text that would read as a marker or end a chunk survives
      const marks = [{ type: 'markdown', source: 'a "quoted" word, "" and a ! and !!\n"%%"' },
                     { type: 'code', source: '"%%"\n1 + 1.\n\'"%% [markdown]"\'.\n#(1 $! 2) size' },
                     { type: 'code', source: '"%% [markdown]" 3' }];
      const back = S.fromSt(S.toSt({ title: 'marks', cells: marks })).cells;
      // a file of plain chunks: a cell each; CRLFs
      const plain = S.fromSt('3 + 4!\r\nTranscript show: \'a\'\r\n  , \'b\'!\r\n').cells;
      const j = L.fromJson(JSON.stringify({ format: 'pharo-notebook', version: 1, meta: {}, cells: [
        { id: 'd1', type: 'code', source: '1', outputs: [{ k: 'value', text: '1' }, { k: 'evil', text: 'x' },
                                                          { k: 'display', mime: 'text/javascript', data: 'x' }] },
        { id: 'd1', type: 'code', source: '2', count: 'x' },
        { type: 'shell', source: 'rm' }, { id: 'ok', type: 'markdown', source: 7 }] }));
      let other = null;
      try { L.fromJson(JSON.stringify({ format: 'chicken-notebook', version: 1, cells: [] })); } catch (e) { other = String(e.message || e); }
      return { marks: JSON.stringify(back.map(c => [c.type, c.source])) === JSON.stringify(marks.map(c => [c.type, c.source])),
               back: back.map(c => [c.type, c.source]), plain: plain.map(c => c.type + ':' + c.source),
               ids: j.cells.map(c => c.id), outs: j.cells[0].outputs.length, count: 'count' in j.cells[1], n: j.cells.length, other };
    });
    assert(r.marks, 'marker-like text round trips: ' + JSON.stringify(r.back));
    assert(r.plain.join('|') === "code:3 + 4|code:Transcript show: 'a'\n  , 'b'", 'plain chunks ' + JSON.stringify(r.plain));
    assert(r.n === 2 && r.ids[0] === 'd1' && r.ids[1] !== 'd1' && r.outs === 1 && !r.count, 'json ' + JSON.stringify(r));
    assert(r.other, 'a notebook of another format is refused');
  });

  await check('import limits: more than 5000 cells are refused, from .st and .json', async () => {
    const n = await nb.cells();
    const chunks = k => Array.from({ length: k }, (_, i) => i + ' + 1!').join('\n') + '\n';
    const json = k => JSON.stringify({ format: 'pharo-notebook', version: 1, meta: {},
                                       cells: Array.from({ length: k }, (_, i) => ({ type: 'code', source: i + ' + 1' })) });
    for (const [name, text] of [['many.st', chunks(5001)], ['many.json', json(5001)]]) {
      await page.evaluate(() => { document.getElementById('nb-notice-text').textContent = ''; });
      await accepting(() => page.setInputFiles('#nb-file', { name, mimeType: 'text/plain', buffer: Buffer.from(text) }));
      await nb.noticeSays(new RegExp('Could not import ' + name.replace('.', '\\.') + ': more than 5000 cells'));
      assert(await nb.cells() === n, name + ': the notebook is kept');
    }
    await page.click('#nb-notice-close');
  });

  // a lone \r is a line break in the editor, so it is one in the overlay;
  // the fences of text cells are colored, and code cells painted, when they
  // come near the visible part of the notebook
  await check('imported sources are colored as the editor shows them, when they come into view', async () => {
    // the notebook as it was, imported again at the end
    await page.keyboard.press('Control+s');
    const was = await page.evaluate(k => localStorage.getItem(k), NB_KEY), n = await nb.cells();
    const cells = [{ type: 'code', source: 'a := 1.\r"c" b := 2.\r\nc := 3' }, { type: 'markdown', source: '```\nself foo\n```' }]
      .concat(Array.from({ length: 300 }, (_, k) => ({ type: 'code', source: k + ' + 1' })),
              [{ type: 'markdown', source: '```st\nnil isNil\n```' }]);
    const buf = Buffer.from(JSON.stringify({ format: 'pharo-notebook', version: 1, meta: { title: 'cr' }, cells }));
    await accepting(() => page.setInputFiles('#nb-file', { name: 'cr.json', mimeType: 'application/json', buffer: buf }));
    await page.waitForFunction(() => document.querySelectorAll('#nb-cells > li').length === 303, null, { timeout: 20000 });
    await page.$eval('#panel-notebook', e => { e.scrollTop = 0; });
    await page.waitForFunction(() => document.querySelector('#nb-cells > li:nth-child(2) .nb-md .syn-special'), null, { timeout: 5000 });
    const r = await page.evaluate(() => {
      const li = document.querySelectorAll('#nb-cells > li'), hl = li[0].querySelector('.nb-hl');
      return { src: li[0].querySelector('.nb-src').value, lines: [...hl.children].map(d => d.textContent),
               comments: [...hl.querySelectorAll('.syn-comment')].map(e => e.textContent),
               far: li[302].querySelectorAll('.nb-md span').length, farCode: li[300].querySelectorAll('.nb-hl span').length,
               painted: document.querySelectorAll('#nb-cells .nb-editor.syn').length };
    });
    assert(r.src === 'a := 1.\n"c" b := 2.\nc := 3' && r.lines.join('|') === 'a := 1.|"c" b := 2.|c := 3' && r.comments.join() === '"c"',
           'line breaks ' + JSON.stringify(r));
    assert(r.far === 0 && r.farCode === 0 && r.painted > 0 && r.painted < 300, 'cells out of sight are not colored yet: ' + JSON.stringify(r));
    await nb.cell(302).scrollIntoViewIfNeeded();
    await page.waitForFunction(() => document.querySelector('#nb-cells > li:nth-child(303) .nb-md .syn-constant') &&
                               document.querySelector('#nb-cells > li:nth-child(301) .nb-editor.syn .syn-number'), null, { timeout: 5000 });
    await accepting(() => page.setInputFiles('#nb-file', { name: 'was.json', mimeType: 'application/json', buffer: Buffer.from(was) }));
    await page.waitForFunction(n => document.querySelectorAll('#nb-cells > li').length === n, n, { timeout: 20000 });
  });

  // Export .html: a static page of what the notebook shows, which loads
  // nothing and runs nothing, opened from file://; hostile outputs stay in
  // their cell or are left out with a note
  await check('Export .html is a static page of the notebook, from file:// too', async () => {
    await page.keyboard.press('Control+s');
    const was = await page.evaluate(k => localStorage.getItem(k), NB_KEY), n = await nb.cells();
    const png = 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==';
    const cells = [
      { type: 'markdown', source: '# Export </title><script>x</script> test\n\n- one\n- two\n\n[to the end](#the-end)\n\n```smalltalk\nx := \'s\'. "c"\n```\n\n```scheme\n(define y 1)\n```' },
      { type: 'code', source: 'Transcript show: \'out\'', count: 1, outputs: [{ k: 'stream', name: 'stdout', text: 'out\n' },
                                                                              { k: 'stream', name: 'stderr', text: 'err\n' }] },
      { type: 'code', source: '1 to: 40', count: 2,
        outputs: [{ k: 'value', text: Array.from({ length: 40 }, (_, k) => 'line ' + k).join('\n') }] },
      { type: 'code', source: '#() first', count: 3, status: 'error', outputs: [{ k: 'error', error: { text: 'Error: Index 1 is out of bounds',
        chain: [{ where: 'Array(Object)>>errorSubscriptBounds:', form: 'self errorSubscriptBounds: index' }, { where: 'In[3]:1', form: '#() first' }] } }] },
      { type: 'code', source: 'Notebook show: x', count: 4, outputs: [
        { k: 'display', mime: 'image/svg+xml', data: '<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"><rect width="40" height="20" style="fill: var(--accent)"/></svg>' },
        { k: 'display', mime: 'text/html', data: '<p class="x" onclick="alert(1)">rich <b>html</b> <a href="https://pharo.org/">link</a> ' +
                                                 '<img src="https://example.com/x.png" alt="ext"><img src="data:image/png;base64,' + png + '" alt="dot"></p>' },
        { k: 'display', mime: 'text/markdown', data: '### Shown\n\n- a\n- b\n\n```\nnil\n```' },
        { k: 'display', mime: 'image/png', data: png },
        { k: 'note', text: '… 10 characters omitted …' }] },
      { type: 'code', source: 'Transcript show: \'hidden\'', count: 5, outputs: [{ k: 'stream', name: 'stdout', text: 'hidden text\n' }] },
      // text the parser would change: a newline right after <pre>, a CR
      { type: 'code', source: 'Transcript cr', count: 6, outputs: [{ k: 'stream', name: 'stdout', text: '\nafter newline\n10%\r20%\r30%\n' },
                                                                   { k: 'display', mime: 'text/plain', data: '\nplain' }] },
      // markup that must not get out of its cell: an <li> with no list, an
      // HTML element in SVG (XML only), and (below) a tree that the parser
      // would build otherwise
      { type: 'code', source: 'Notebook html: x', count: 7, outputs: [
        { k: 'display', mime: 'text/html', data: '<li>item</li>' },
        { k: 'display', mime: 'text/html', data: '<div><li style="position:fixed;top:0;left:0;width:100vw;height:100vh;z-index:99">ESCAPED</li></div>' },
        { k: 'display', mime: 'image/svg+xml', data: '<svg xmlns="http://www.w3.org/2000/svg" xmlns:h="http://www.w3.org/1999/xhtml" width="10" height="10">' +
                                                     '<rect width="10" height="10"/><h:li>svg li</h:li></svg>' },
        { k: 'display', mime: 'text/html', data: '<p>replaced</p>' }] },
    ].concat(Array.from({ length: 200 }, (_, k) => ({ type: 'code', source: k + ' + 1' })),
             [{ type: 'code', source: 'x := self far: \'far\'' },
              { type: 'code', source: '#(' + '\'0123456789\' '.repeat(3000) + ')' },
              { type: 'markdown', source: '## The end\n\n```\nself foo\n```' }]);
    const buf = Buffer.from(JSON.stringify({ format: 'pharo-notebook', version: 1, meta: { title: 'html export' }, cells }));
    await accepting(() => page.setInputFiles('#nb-file', { name: 'ex.json', mimeType: 'application/json', buffer: buf }));
    await page.waitForFunction(n => document.querySelectorAll('#nb-cells > li').length === n, cells.length, { timeout: 20000 });
    await page.$eval('#panel-notebook', e => { e.scrollTop = 0; });
    await nb.cell(5).focus();
    await page.keyboard.press('o');                       // its output hidden
    await nb.src(1).fill('Transcript show: \'out!\'');      // edited since it ran
    await nb.cell(0).locator('.nb-md').dblclick();        // the text cell in edit mode
    const inApp = await page.evaluate(() => {
      const li = document.querySelectorAll('#nb-cells > li')[7], rich = li.querySelectorAll('.nb-rich');
      // ul > li > div > li: read back, the inner <li> closes the outer one
      const ul = rich[3].appendChild(document.createElement('ul')), o = ul.appendChild(document.createElement('li'));
      o.appendChild(document.createElement('div')).appendChild(document.createElement('li')).textContent = 'inner';
      const st = li.previousElementSibling.querySelector('.nb-stream');
      return [st.textContent, li.previousElementSibling.querySelector('.nb-rich pre').textContent, rich[2].textContent, st.getBoundingClientRect().height];
    });
    assert(inApp[0] === '\nafter newline\n10%\r20%\r30%\n' && inApp[1] === '\nplain' && inApp[2] === '', 'in the page ' + JSON.stringify(inApp));
    const far = cells.length - 3;
    assert(!await nb.cell(far).locator('.nb-editor.syn').count(), 'the far cell is not painted in the page');
    const [dl] = await Promise.all([page.waitForEvent('download'), nb.menu('export-html')]);
    assert(dl.suggestedFilename() === 'html-export.html', 'file name ' + dl.suggestedFilename());
    // saved as .html: file:// goes by the extension
    const file = path.join(fs.mkdtempSync(path.join(os.tmpdir(), 'nb-export-')), dl.suggestedFilename());
    await dl.saveAs(file);
    const text = fs.readFileSync(file, 'utf8');
    assert(/^<!doctype html>\n<html lang="en"><head><meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; img-src data:; font-src data:;/.test(text),
           'head: ' + text.slice(0, 300));
    assert(!/<script|<link|<iframe|<object|@import|url\((?!#)/i.test(text), 'no script, link or external CSS');
    await page.waitForFunction(() => document.getElementById('nb-live').textContent === 'Exported html-export.html', null, { timeout: 5000 });

    // loaded from file://, as a page of its own
    const f = await t.context.newPage();
    const fErrors = [], requests = [];
    f.on('console', m => { if (m.type() === 'error' || m.type() === 'warning') fErrors.push(m.type() + ': ' + m.text()); });
    f.on('pageerror', e => fErrors.push('pageerror: ' + e.message));
    f.on('request', r => { if (!/^(file|data):/.test(r.url())) requests.push(r.url()); });
    await f.goto('file://' + file);
    const r = await f.evaluate(() => {
      const $ = s => document.querySelector(s), $$ = s => [...document.querySelectorAll(s)];
      const li = $$('.nb-cells > .nb-cell'), col = e => e && getComputedStyle(e).color, box = e => e.getBoundingClientRect();
      const attrs = $$('*').flatMap(e => [...e.attributes].map(a => [e.localName, a.name, a.value]));
      const probe = document.body.appendChild(document.createElement('span'));
      probe.style.color = 'var(--accent)';
      const accent = getComputedStyle(probe).color;
      probe.style.color = 'var(--bad)';
      const bad = getComputedStyle(probe).color;
      probe.remove();
      const count = document.createRange();
      count.selectNodeContents(li[1].querySelector('.nb-count'));
      const svg = $('.nb-rich svg rect');
      return {
        title: document.title, head: $('.nbx-title').textContent, foot: $('.nbx-foot').textContent,
        csp: !!$('meta[http-equiv="Content-Security-Policy"]'),
        handlers: attrs.filter(([, n]) => /^on/i.test(n)).length,
        urls: attrs.filter(([t, n, v]) => (n === 'src' && !v.startsWith('data:image/')) || (n === 'href' && !(t === 'a' && /^(#|https?:|mailto:)/.test(v))) ||
                                          (n === 'style' && /url\((?!#)/.test(v))).map(a => a.join(' ')),
        chrome: $$('button, textarea, input, form, .nb-bal, .nb-tools, .nb-run, .nb-stdin, .nb-hl, .nb-editor').length,
        md: [$('.nb-md h1') && $('.nb-md h1').textContent, $$('.nb-md ul > li').length],
        fence: $$('.nb-md pre code .syn-special').map(e => e.textContent), lastFence: !!li.at(-1).querySelector('pre code .syn-special'),
        scheme: $$('.nb-md pre code')[1] && [$$('.nb-md pre code')[1].textContent, $$('.nb-md pre code')[1].children.length],
        anchor: (() => { const a = $('.nb-md a[href^="#"]'); return a && document.getElementById(a.getAttribute('href').slice(1))?.textContent; })(),
        counts: li.filter(e => e.dataset.type === 'code').slice(0, 6).map(e => e.querySelector('.nb-count').textContent),
        edited: li[1].dataset.edited === 'true' && getComputedStyle(li[1].querySelector('.nb-count')).fontStyle === 'italic',
        stale: li[2].dataset.stale === 'true', src1: li[1].querySelector('.nb-code').textContent,
        streams: [...li[1].querySelectorAll('.nb-stream')].map(e => e.className + ':' + e.textContent),
        value: (() => { const v = li[2].querySelector('.nb-value'); return v && [v.classList.contains('clamped'), v.textContent.split('\n').length, v.scrollHeight <= v.clientHeight + 1]; })(),
        error: [$('.nb-error-msg .tag')?.textContent, $('.nb-error-msg')?.textContent, $$('.nb-trace .where').map(e => e.localName + ':' + e.textContent)],
        svg: svg && [getComputedStyle(svg).fill, svg.getBoundingClientRect().width], accent,
        html: [$$('.nb-rich[data-mime="text/html"] b').length, $$('.nb-rich img').map(i => i.getAttribute('src') ? i.alt.replace(/\d+$/, 'N') + ':' + i.naturalWidth : i.alt + ':none')],
        rmd: [$$('.nb-rich[data-mime="text/markdown"] li').length, $$('.nb-rich[data-mime="text/markdown"] pre code .syn-constant').length],
        note: $$('.nb-note').map(e => e.textContent),
        hidden: (() => { const d = li[5].querySelector('details.nbx-hidden'); return d && [d.open, d.querySelector('summary').textContent, d.querySelector('.nb-out').textContent]; })(),
        far: [li.at(-3).querySelector('.nb-code').textContent, [...li.at(-3).querySelectorAll('.syn-special')].map(e => e.textContent)],
        big: [li.at(-2).querySelector('.nb-code').textContent.length, li.at(-2).querySelectorAll('.nb-code span').length],
        syn: col($('.nb-code .syn-special')), plain: col($('.nb-code')), string: col($('.nb-code .syn-string')),
        wide: document.documentElement.scrollWidth <= document.documentElement.clientWidth,
        cells: [li.length, document.body.children.length, !!$('.nbx > footer.nbx-foot'), $$('.nb-cells > :not(.nb-cell)').length],
        // [n] next to the code, as in the page; the red bar of an error
        right: box(li[1].querySelector('.nb-gutter')).right - count.getBoundingClientRect().right,
        bars: [getComputedStyle(li[3], '::before').backgroundColor, getComputedStyle(li[1], '::before').backgroundColor], bad,
        errBar: getComputedStyle(li[3].querySelector('.nb-error')).borderLeftWidth,
        pre: [li[6].querySelector('.nb-stream').textContent, li[6].querySelector('.nb-rich pre').textContent,
              box(li[6].querySelector('.nb-stream')).height],
        markup: [...li[7].querySelectorAll('.nb-out > *')].map(e => e.className + ':' + e.textContent),
        held: $$('li').filter(e => !/^[uo]l$/.test(e.parentNode.localName) && !e.closest('.nb-rich')).length, fixed: (() => { const e = $$('li').find(e => e.textContent === 'ESCAPED');
                                                                                        return e && !!e.closest('.nb-rich') && getComputedStyle(e).position; })(),
      };
    });
    const j = JSON.stringify(r), T = 'Export </title><script>x</script> test';     // escaped
    assert(r.title === T && r.head === T && r.csp, 'title and CSP ' + j);
    assert(/^Exported from the Pharo notebook on \S/.test(r.foot), 'footer ' + r.foot);
    assert(!r.handlers && !r.urls.length && !r.chrome, 'static, nothing external: ' + j);
    assert(r.md[0] === T && r.md[1] === 2 && r.fence.join() === ':=,self' && r.lastFence && r.anchor === 'The end',
           'markdown (also in edit mode), Smalltalk fences colored, anchors ' + j);
    assert(r.scheme && r.scheme[0] === '(define y 1)' && !r.scheme[1], 'a fence of another language is plain ' + JSON.stringify(r.scheme));
    assert(r.counts.join() === '[1],[2],[3],[4],[5],[6]' && r.edited && r.stale && r.src1 === 'Transcript show: \'out!\'', 'gutter ' + j);
    assert(r.streams.join('|') === 'nb-stream stdout:out\n|nb-stream stderr:err\n', 'streams ' + r.streams);
    assert(r.value && !r.value[0] && r.value[1] === 40 && r.value[2], 'the long value in full ' + JSON.stringify(r.value));
    assert(r.error[0] === 'Error' && /out of bounds/.test(r.error[1]) &&
           r.error[2].join() === 'span:Array(Object)>>errorSubscriptBounds:,span:In[3]:1', 'error ' + JSON.stringify(r.error));
    assert(r.svg && r.svg[0] === r.accent && r.svg[1] === 40, 'svg ' + JSON.stringify(r.svg) + ' ' + r.accent);
    assert(r.html[0] === 1 && r.html[1].join() === 'dot:1,Image output of cell N:1', 'html (the external image dropped), the PNG ' + JSON.stringify(r.html));
    assert(r.rmd[0] === 2 && r.rmd[1] === 1 && r.note.includes('… 10 characters omitted …'), 'markdown output and note ' + j);
    assert(r.hidden && !r.hidden[0] && r.hidden[1] === 'Output hidden' && r.hidden[2] === 'hidden text\n', 'hidden ' + JSON.stringify(r.hidden));
    assert(r.far[0] === 'x := self far: \'far\'' && r.far[1].join() === ':=,self', 'far cell colored ' + JSON.stringify(r.far));
    assert(r.big[0] > 32 * 1024 && r.big[1] === 0, 'a big cell stays plain ' + r.big);
    assert(r.syn !== r.plain && r.string !== r.plain && r.syn !== r.string, 'colors ' + [r.syn, r.string, r.plain]);
    assert(r.wide, 'no horizontal scroll');
    assert(r.cells[0] === cells.length && r.cells[1] === 1 && r.cells[2] && !r.cells[3], 'every cell in its place ' + JSON.stringify(r.cells));
    assert(r.right >= 0 && r.right <= 4, 'the count is right-aligned: ' + r.right);
    assert(r.bars[0] === r.bad && r.bars[1] !== r.bad && r.errBar === '3px', 'status bar and error bar ' + JSON.stringify([r.bars, r.bad, r.errBar]));
    assert(r.pre[0] === '\nafter newline\n10% 20% 30%\n' && r.pre[1] === '\nplain' && Math.abs(r.pre[2] - inApp[3]) < 1,
           'a leading newline and a CR as in the page ' + JSON.stringify([r.pre, inApp]));
    assert(r.markup.join('|') === 'nb-rich:item|nb-rich:ESCAPED|nb-rich:|nb-note bad:output left out: its markup does not read back the same in a static page' &&
           !r.held && r.fixed === 'fixed', 'markup kept in its cell ' + JSON.stringify(r.markup) + ' ' + r.held + ' ' + r.fixed);
    // the dark scheme, print (light whatever the scheme) and phones
    const look = () => f.evaluate(() => [getComputedStyle(document.body).backgroundColor, getComputedStyle(document.querySelector('.nb-code .syn-special')).color,
                                         document.documentElement.scrollWidth <= document.documentElement.clientWidth]);
    const light = await look();
    await f.emulateMedia({ colorScheme: 'dark' });
    const dark = await look();
    await f.emulateMedia({ media: 'print', colorScheme: 'dark' });
    const print = await look();
    await f.emulateMedia({ media: 'screen', colorScheme: 'light' });
    const phones = [];
    for (const width of [390, 320]) {
      await f.setViewportSize({ width, height: 800 });
      phones.push((await look())[2]);
    }
    assert(dark[0] !== light[0] && dark[1] !== light[1], 'dark ' + JSON.stringify([light, dark]));
    assert(print[1] === light[1] && print[0] !== dark[0], 'print is light ' + JSON.stringify([print, dark]));
    assert(phones.every(Boolean), 'no horizontal scroll at 390 and 320 px ' + phones);
    assert(!fErrors.length && !requests.length, 'the export page: ' + fErrors.concat(requests).join('\n'));
    await f.close();
    fs.rmSync(path.dirname(file), { recursive: true, force: true });
    await accepting(() => page.setInputFiles('#nb-file', { name: 'was.json', mimeType: 'application/json', buffer: Buffer.from(was) }));
    await page.waitForFunction(n => document.querySelectorAll('#nb-cells > li').length === n, n, { timeout: 20000 });
  });

  // a big notebook is colored up to 1 MB of sources in all, the rest
  // plain, so that the export does not freeze the page for seconds
  await check('Export .html colors at most 1 MB of sources', async () => {
    await page.keyboard.press('Control+s');
    const was = await page.evaluate(k => localStorage.getItem(k), NB_KEY), n = await nb.cells();
    const unit = 'self g: 1 > 2 ifTrue: [\'s\'] ifFalse: [$a]. "c"\n', src = unit.repeat(Math.floor(31000 / unit.length));
    const cells = Array.from({ length: 40 }, () => ({ type: 'code', source: src }));
    const buf = Buffer.from(JSON.stringify({ format: 'pharo-notebook', version: 1, meta: { title: 'big export' }, cells }));
    await accepting(() => page.setInputFiles('#nb-file', { name: 'big.json', mimeType: 'application/json', buffer: buf }));
    await page.waitForFunction(n => document.querySelectorAll('#nb-cells > li').length === n, cells.length, { timeout: 20000 });
    const t0 = Date.now();
    const [dl] = await Promise.all([page.waitForEvent('download'), nb.menu('export-html')]);
    const ms = Date.now() - t0;
    const text = fs.readFileSync(await dl.path(), 'utf8');
    const code = text.split('<pre class="nb-code"><code>').slice(1).map(t => t.startsWith('<span') ? 'c' : 'p').join('');
    const k = Math.floor(1024 * 1024 / src.length);
    assert(code === 'c'.repeat(k) + 'p'.repeat(40 - k), 'colored ' + code);
    console.log('  # Export .html of 40 cells of ' + src.length + ' characters: ' + ms + ' ms, ' + text.length + ' characters');
    await accepting(() => page.setInputFiles('#nb-file', { name: 'was.json', mimeType: 'application/json', buffer: Buffer.from(was) }));
    await page.waitForFunction(n => document.querySelectorAll('#nb-cells > li').length === n, n, { timeout: 20000 });
  });

  // The other tab saved, this one autosaved over it before the choice was
  // made: Load theirs still loads theirs
  await check('Load theirs loads what another tab saved', async () => {
    await page.keyboard.press('Control+s');
    const setLast = (p, v) => p.evaluate(v => {
      const t = [...document.querySelectorAll('#nb-cells > li[data-type="code"] .nb-src')].pop();
      t.value = v;
      t.dispatchEvent(new Event('input'));
    }, v);
    const saved = (p, re) => p.waitForFunction(([k, s, f]) => new RegExp(s, f).test(localStorage.getItem(k)),
                                               [NB_KEY, re.source, re.flags], { timeout: 5000 });
    const f = await t.context.newPage();
    t.watch(f);
    await f.goto(base + 'index.html');
    await f.click('#tab-notebook');
    await setLast(f, "'theirs'");
    await saved(f, /'theirs'/);
    await f.close();
    await nb.noticeSays(/changed in another tab/, 5000);
    await setLast(page, "'mine'");
    await saved(page, /'mine'/);
    await page.click('#nb-notice-action');
    const last = await page.$$eval('#nb-cells > li[data-type="code"] .nb-src', l => l.map(t => t.value).pop());
    assert(last === "'theirs'", 'loaded: ' + last);
    await saved(page, /'theirs'/);
    assert(Object.keys(await page.evaluate(() => ({ ...localStorage }))).filter(k => /notebook/.test(k)).join() === NB_KEY,
           'one autosave key: ' + Object.keys(await page.evaluate(() => ({ ...localStorage }))));
    await nb.waitState(/^ready$/);
  });

  await check('Ctrl+C on a selected output copies; the running cell goes on', async () => {
    const a = await nb.eval("Transcript show: 'copy me'; cr");
    const b = await nb.add('[ (Delay forSeconds: 1) wait ] repeat');
    await nb.run(b);
    await nb.wait(b, /^running$/);
    await nb.cell(a).locator('.nb-stream').click({ clickCount: 3 });
    const sel = await page.evaluate(() => [String(getSelection()), document.activeElement.matches('#nb-cells > li')]);
    assert(/copy me/.test(sel[0]) && sel[1], 'the output selected, the cell focused: ' + sel);
    await page.keyboard.press('Control+c');
    await page.waitForTimeout(500);
    assert(await nb.status(b) === 'running', 'still running: ' + await nb.status(b));
    await nb.src(b).click();
    await nb.src(b).press('Control+c');
    await nb.wait(b, /^interrupted$/, 5000);
  });

  await check('a saved status and fragment links survive an import', async () => {
    const s = await nb.add('[ true ] whileTrue');
    await nb.run(s);
    await nb.wait(s, /^running$/);
    await page.click('#nb-stop');
    await nb.wait(s, /^interrupted$/, 5000);
    for (const src of ['# Tips\n\n[to the tips](#tips)', '# Tips']) {
      await page.click('#nb-end-text');
      const m = (await nb.cells()) - 1;
      await nb.src(m).fill(src);
      await nb.src(m).press('Shift+Enter');
    }
    const links = () => page.evaluate(() => {
      const a = [...document.querySelectorAll('#nb-cells .nb-md a[href^="#"]')].pop();
      const ids = [...document.querySelectorAll('#nb-cells .nb-md [id]')].map(e => e.id);
      return { to: a && document.querySelector(a.getAttribute('href'))?.textContent, unique: new Set(ids).size === ids.length };
    });
    const before = await links();
    assert(before.to === 'Tips' && before.unique, 'links ' + JSON.stringify(before));
    const [dl] = await Promise.all([page.waitForEvent('download'), nb.menu('export-json')]);
    const text = fs.readFileSync(await dl.path(), 'utf8');
    await accepting(() => page.setInputFiles('#nb-file', { name: 'nb.json', mimeType: 'text/plain', buffer: Buffer.from(text) }));
    assert(await nb.status(s) === 'interrupted', 'the status after the import: ' + await nb.status(s));
    const after = await links();
    assert(after.to === 'Tips' && after.unique, 'links after the import ' + JSON.stringify(after));
  });

  await check('the undo of a delete does not reach into an imported notebook', async () => {
    const [dl] = await Promise.all([page.waitForEvent('download'), nb.menu('export-json')]);
    const text = fs.readFileSync(await dl.path(), 'utf8');
    const n = await nb.cells();
    await nb.cell(1).locator('[data-act="del"]').click();
    await accepting(() => page.setInputFiles('#nb-file', { name: 'nb.json', mimeType: 'text/plain', buffer: Buffer.from(text) }));
    await page.waitForFunction(n => document.querySelectorAll('#nb-cells > li').length === n, n);
    assert(await page.isHidden('#nb-toast'), 'the Undo toast is gone');
    await nb.cell(0).focus();
    await page.keyboard.press('z');
    await page.waitForTimeout(200);
    const ids = await page.$$eval('#nb-cells > li', l => l.map(e => e.dataset.id));
    assert(ids.length === n && new Set(ids).size === n, 'cells ' + ids.length + ' of ' + n + ', unique ids');
    await page.keyboard.press('Control+s');
    const saved = await page.evaluate(k => JSON.parse(localStorage.getItem(k)).cells.length, NB_KEY);
    assert(saved === n, 'saved cells ' + saved);
  });

  await check('the shortcuts dialog fits at 320 px', async () => {
    await page.setViewportSize({ width: 320, height: 740 });
    await nb.cell(0).focus();
    await page.keyboard.press('?');
    await page.waitForSelector('#nb-shortcuts[open]');
    const w = await page.$eval('#nb-shortcuts', d => [d.scrollWidth, d.clientWidth, d.querySelector('.keys').scrollWidth,
                                                       d.querySelector('.keys').clientWidth]);
    await page.keyboard.press('Escape');
    await page.setViewportSize({ width: 1280, height: 800 });
    assert(w[0] <= w[1] && w[2] <= w[3], 'no horizontal scroll: ' + w);
  });

  await check('every button and menu item has an accessible name', async () => {
    const nameless = await page.evaluate(() => [...document.querySelectorAll(
      '[role="tab"], #panel-notebook button, #nb-toolbar button, #nb-menu [role="menuitem"], #nb-shortcuts button')]
      .filter(b => !(b.getAttribute('aria-label') || b.textContent.trim() || b.title || b.getAttribute('aria-labelledby')))
      .map(b => b.outerHTML.slice(0, 80)));
    assert(!nameless.length, nameless.join('\n'));
    const panel = await page.$eval('#panel-notebook', e => [e.getAttribute('role'), e.getAttribute('aria-labelledby')]);
    assert(panel[0] === 'tabpanel' && panel[1] === 'tab-notebook', 'the panel ' + panel);
  });

  await check('autosave across a reload, also of a cell over 1 MB; storage that fails says so', async () => {
    const i = await nb.eval("persisted := 1. 'persist me'");
    await page.keyboard.press('Control+s');
    await page.reload();
    await page.click('#tab-notebook');
    const k = await page.$$eval('#nb-cells > li', l => l.findIndex(e => /persist me/.test(e.querySelector('.nb-src').value)));
    assert(k === i, 'the source restored at ' + k);
    assert(await nb.cell(k).getAttribute('data-stale') === 'true' && /persist me/.test(await nb.out(k)), 'the output stale');
    assert(await page.isVisible('#nb-notice') && /Restored/.test(await nb.notice()), 'the restored notice');
    await nb.waitState(/^ready$/);
    await started();
    // a cell larger than 1 MB, which autosave keeps, is restored too
    const b = await nb.add('"' + 'x'.repeat(1100000) + '"');
    await nb.add('afterBig := 2');
    await page.keyboard.press('Control+s');
    const n = await nb.cells();
    await page.reload();
    await page.click('#tab-notebook');
    const big = await page.$$eval('#nb-cells > li .nb-src', l => l.map(e => e.value.length));
    assert(big.length === n && big[b] === 1100002, 'cells after the reload: ' + big.length + ' of ' + n + ', big ' + big[b]);
    assert(!/skipped/.test(await nb.notice()), 'notice: ' + await nb.notice());
    await nb.cell(b).locator('[data-act="del"]').click();
    await page.keyboard.press('Control+s');
    await nb.waitState(/^ready$/);
    await started();
    // no localStorage at all, then a full one
    for (const mode of ['blocked', 'full']) {
      const f = await t.context.newPage();
      const errs = [];
      f.on('console', m => { if (m.type() === 'error') errs.push(m.text()); });
      f.on('pageerror', e => errs.push(e.message));
      if (mode === 'blocked') await f.addInitScript(() => {
        Object.defineProperty(window, 'localStorage', { get() { throw new DOMException('denied', 'SecurityError'); } });
      });
      else await f.addInitScript(() => {
        const set = Storage.prototype.setItem;
        Storage.prototype.setItem = function (k, v) {
          if (String(v).length > 1000) throw new DOMException('full', 'QuotaExceededError');
          return set.call(this, k, v);
        };
      });
      await f.goto(base + 'index.html');
      await f.click('#tab-notebook');
      await f.waitForFunction(() => document.getElementById('nb-status').dataset.state === 'ready', null, { timeout: 90000 });
      await f.click('#nb-end-code');
      await f.keyboard.type('20 + 22');
      await f.keyboard.press('Control+Enter');
      await f.waitForFunction(() => [...document.querySelectorAll('.nb-value')].some(e => e.textContent === '42'), null, { timeout: 30000 });
      await f.waitForFunction(() => /Autosave failed/.test(document.getElementById('nb-notice-text').textContent) &&
                              !document.getElementById('nb-notice').hidden, null, { timeout: 5000 });
      assert(!errs.length, mode + ': ' + errs.join('\n'));
      await f.close();
    }
  });

  // ---- the image of the Console

  await check('after a Save of the Console, the notebook says so; a Restart boots that image, fetched from no server', async () => {
    await page.click('#tab-console');
    await evalIn(page, 'Smalltalk at: #NbSavedMark put: 31', /\n31\nst> $/);
    await page.evaluate(() => { document.getElementById('notice-text').textContent = ''; });
    await page.click('#save');
    await noticeSays(/saved in this browser/, page, 90000);
    await waitState('waiting');
    await page.click('#tab-notebook');
    await nb.noticeSays(/image changed/i);
    const n = t.requests.length, inits = await notebookInits();
    await page.click('#nb-notice-action');
    await nb.waitState(/^ready$/);
    assert(await notebookInits() === inits + 1, 'restarted');
    const i = await nb.eval('Smalltalk at: #NbSavedMark ifAbsent: [ #none ]');
    assert((await nb.values(i)).join() === '31', 'the kernel booted the saved image: ' + await nb.out(i));
    const served = t.requests.slice(n);
    assert(!served.includes(imageUrl) && !served.includes(changesUrl), 'fetched ' + served.join(' '));
    console.log('  # the kernel of the saved image fetched the .sources ' + served.filter(u => u === sourcesUrl).length + ' time(s)');
  });

  await check('a snapshot in a cell leaves the image saved in this browser as it was', async () => {
    // (once the Console has stored what it stores after a save)
    let before = null;
    for (let k = 0, last = null; k < 20; k++, await page.waitForTimeout(500)) {
      const m = JSON.stringify(await slotMeta());
      if (m === last) { before = JSON.parse(m); break; }
      last = m;
    }
    assert(before && before.imageSize > 1e7, 'a slot: ' + JSON.stringify(before));
    await page.evaluate(() => { document.getElementById('notice-text').textContent = ''; });
    const i = await nb.eval('Smalltalk snapshot: true andQuit: false. 3 + 4', 90000);
    assert(await nb.status(i) === 'ok', 'the snapshot cell: ' + await nb.out(i));
    const j = await nb.eval('6 * 7');
    assert((await nb.values(j)).join() === '42', 'the next cell: ' + await nb.out(j));
    const k = await nb.eval('[ 1/0 ] fork. (Delay forMilliseconds: 300) wait. #alive');
    assert((await nb.values(k)).join() === '#alive', 'the kernel lives on: ' + await nb.out(k));
    await page.waitForTimeout(1000);
    assert(JSON.stringify(await slotMeta()) === JSON.stringify(before), 'the slot ' + JSON.stringify(await slotMeta()));
    assert(!/saved in this browser/.test(await page.textContent('#notice-text')), 'the Console: ' + await page.textContent('#notice-text'));
  });

  await check('after a Reset of the Console, the notebook says so', async () => {
    await page.evaluate(() => { document.getElementById('nb-notice-text').textContent = ''; });
    await page.click('#tab-console');
    await accepting(() => page.click('#reset'));
    await page.waitForFunction(() => /Deleted the image saved in this browser[\s\S]*st> $/.test(document.getElementById('term').textContent),
                               null, { timeout: 90000 });
    await page.click('#tab-notebook');
    await nb.noticeSays(/image changed/i);
    await page.click('#nb-notice-action');
    await nb.waitState(/^ready$/);
    const i = await nb.eval('Smalltalk at: #NbSavedMark ifAbsent: [ #none ]');
    assert((await nb.values(i)).join() === '#none', 'the site\'s image: ' + await nb.out(i));
  });

  await check('two VMs in one page: the memory, logged', async () => {
    const m = await page.evaluate(async () => {
      if (!self.crossOriginIsolated || !performance.measureUserAgentSpecificMemory)
        return 'not measured (the page is not cross-origin isolated)';
      return Math.round((await performance.measureUserAgentSpecificMemory()).bytes / 1e6) + ' MB';
    });
    assert(vmWorkers() === 2, 'two VM workers: ' + vmWorkers());
    console.log('  # the memory of the page with two VMs: ' + m);
  });

  // ---- lockouts

  await check('from file:// the notebook says it needs HTTP, and starts no worker', async () => {
    const f = await t.context.newPage();
    t.watch(f);
    await f.goto('file://' + path.join(t.webDir, 'index.html'));
    await f.waitForFunction(() => document.getElementById('status').dataset.state === 'error', null, { timeout: 10000 });
    await f.setViewportSize({ width: 320, height: 640 });
    await f.click('#tab-notebook');
    const notice = () => f.textContent('#nb-notice-text');
    assert(/served over HTTP/.test(await notice()) && await f.isVisible('#nb-notice'), 'nb notice: ' + await notice());
    assert(await f.isDisabled('#nb-run-all') && await f.isDisabled('#nb-restart'), 'Run all and Restart disabled');
    await f.locator('#nb-cells > li[data-type="code"] .nb-src').first().press('Shift+Enter');
    await f.waitForTimeout(500);
    assert(!f.workers().length, 'no worker: ' + f.workers().map(w => w.url()));
    assert(await f.textContent('#nb-status-text') === 'unavailable', 'status: ' + await f.textContent('#nb-status-text'));
    const panel = await f.$eval('#panel-notebook', e => [e.scrollWidth, e.clientWidth]);
    assert(panel[0] <= panel[1], 'the notebook scrolls sideways: ' + panel);
    await f.click('#tab-console');
    await f.close();
  });

  await check('without memory64 neither the Console nor the notebook starts a worker; both say why', async () => {
    const f = await t.context.newPage();
    t.watch(f);
    // an engine without memory64, as Safari's: WebAssembly.validate rejects
    // the probe of page.js (a memory section of a 64-bit memory), and the
    // compile of pharo-web.wasm fails.  The page opens on the Notebook tab,
    // whose kernel then asks for the module as the Console does
    await f.addInitScript(() => {
      try { localStorage.setItem('pharo-wasm.tab', 'notebook'); } catch (e) { /* the default tab then */ }
      const validate = WebAssembly.validate;
      WebAssembly.validate = function (bytes) {
        const b = new Uint8Array(bytes.buffer || bytes);
        for (let i = 8; i + 3 < b.length; i++)
          if (b[i] === 0x05 && b[i + 2] === 0x01 && (b[i + 3] & ~1) === 0x04) return false;
        return validate.apply(this, arguments);
      };
      const no = () => Promise.reject(new WebAssembly.CompileError('memory64 is not supported'));
      WebAssembly.compile = no;
      WebAssembly.compileStreaming = no;
    });
    await f.goto(base + 'index.html');
    await f.waitForFunction(() => document.getElementById('status').dataset.state === 'error', null, { timeout: 30000 });
    await f.waitForFunction(() => /memory64/.test(document.getElementById('nb-notice-text').textContent), null, { timeout: 10000 });
    assert(await f.isVisible('#panel-notebook') && await f.isVisible('#nb-notice'), 'the notice of the notebook shows');
    await f.locator('#nb-cells > li[data-type="code"] .nb-src').first().press('Shift+Enter');
    await f.waitForTimeout(500);
    assert(!f.workers().length, 'workers: ' + f.workers().map(w => w.url()));
    assert(await notebookInits(f) === 0, 'a notebook worker: ' + JSON.stringify(await posts(f)));
    assert(await f.textContent('#nb-status-text') === 'unavailable', 'status: ' + await f.textContent('#nb-status-text'));
    assert(await f.isDisabled('#nb-run-all'), 'Run all enabled');
    await f.click('#tab-console');
    assert(/memory64/.test(await f.textContent('#notice-text')), 'the Console: ' + await f.textContent('#notice-text'));
    await f.close();
  });

  // (last: it leaves the image it opened in this browser)
  if (process.env.NB_OPEN_ZIP) await check('a Pharo zip opened in the Console: the kernel boots it, Run all on the example passes', async () => {
    const zip = path.resolve(process.env.NB_OPEN_ZIP);
    await page.click('#tab-console');
    const at = (await term()).length;
    // (the Console asks before it unpacks a zip)
    const yes = d => d.accept().catch(() => {});
    page.on('dialog', yes);
    try {
      await page.setInputFiles('#open-file', zip);
      await page.waitForFunction(at => /"Started the opened image, [^"]*"\n[\s\S]*st> $/.test(document.getElementById('term').textContent.slice(at)),
                                 at, { timeout: 180000 });
      await noticeSays(/The opened image is now kept in this browser|cannot open the Pharo world/, page, 60000);
    } finally { page.off('dialog', yes); }
    await page.evaluate(() => { document.getElementById('nb-notice-text').textContent = ''; });
    await page.click('#tab-notebook');
    await nb.noticeSays(/image changed/i);
    await page.click('#nb-notice-action');
    await nb.waitState(/^ready$/, 120000);
    const info = await page.textContent('#nb-info');
    assert(/^Pharo 1[25]\.\d+ \u00b7 wasm64$/.test(info), 'info ' + info);
    console.log('  # ' + path.basename(zip) + ': ' + info);
    await accepting(() => nb.menu('example'));
    await page.waitForFunction(() => document.querySelector('#nb-cells > li') &&
                               document.querySelector('#nb-cells > li').dataset.type === 'markdown' &&
                               [...document.querySelectorAll('#nb-cells > li[data-type="code"]')].every(e => !e.dataset.status || e.dataset.status === 'idle'),
                               null, { timeout: 10000 });
    await runExample(page);
  });

  await check('no Content-Security-Policy violation; the policy of the page', async () => {
    const meta = await page.$eval('meta[http-equiv="Content-Security-Policy"]', e => e.getAttribute('content'));
    assert(meta === CSP, 'the policy: ' + meta);
    assert(!csp.length, csp.slice(0, 5).join(' | '));
  });

  await check('no worker warned of an engine error or of a callback that threw', async () => {
    assert(!diag.length, diag.slice(0, 5).join(' | '));
  });
});
