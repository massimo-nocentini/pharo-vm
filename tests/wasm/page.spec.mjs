// page.spec.mjs - browser test of the Console page (build-wasm/web/index.html)
//
// usage: node page.spec.mjs WEB_DIR
//
// A plain node script, run by `make wasm-check-browser' and never a build
// dependency.  tests/wasm/lib/pw.mjs serves WEB_DIR (no COOP/COEP headers)
// and drives the page in each browser named by BROWSERS (default
// "chromium"); see there for PLAYWRIGHT_MODULE and SHOTS, which saves light
// and dark screenshots at 1280 and 360 px.  Each browser gets a context of
// its own, so its IndexedDB starts empty.  Exits with status 1 if any check
// fails or the page logs an error.

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { run } from './lib/pw.mjs';

const tmp = fs.mkdtempSync(path.join(process.env.TEST_DIR || os.tmpdir(), 'page-spec-'));
process.on('exit', () => fs.rmSync(tmp, { recursive: true, force: true }));

await run(async t => {
  const { page, check, assert, manifest } = t;
  const imageUrl = '/' + manifest.files.find(f => f.path === manifest.image).url;
  const changesUrl = '/' + manifest.files.find(f => f.path === manifest.image.replace(/\.image$/, '.changes')).url;
  const sourcesUrl = '/' + manifest.files.find(f => f.path.endsWith('.sources')).url;

  const term = () => page.$eval('#term', e => e.textContent);
  const waitStatus = (st, timeout = 30000) =>
    page.waitForFunction(s => document.getElementById('status').dataset.state === s, st, { timeout });
  // text after the last mark, so earlier output never satisfies a check
  let markAt = 0;
  const mark = async () => { markAt = (await term()).length; };
  const since = async () => (await term()).slice(markAt);
  const waitSince = (re, timeout = 30000) =>
    page.waitForFunction(([src, fl, at]) => new RegExp(src, fl).test(document.getElementById('term').textContent.slice(at)),
                         [re.source, re.flags, markAt], { timeout });
  async function enter(text) {
    await mark();
    await page.fill('#line', text);
    await page.press('#line', 'Enter');
  }
  async function evalTo(text, re, timeout) {
    await enter(text);
    await waitSince(re, timeout);
    await waitStatus('waiting', timeout);
  }
  // waits for the prompt of a fresh VM
  const started = async (timeout = 90000) => {
    await waitStatus('waiting', timeout);
    await waitSince(/(^|\n)st> $/, timeout);
  };
  const reload = async () => { markAt = 0; await page.reload(); await started(); };
  // an evaluation runs: the page says so at once, the worker after a slice
  const waitBusy = () => page.waitForFunction(() => /^(running|busy)$/.test(document.getElementById('status').dataset.state));
  const requestsSince = n => t.requests.slice(n);
  // The page's confirm dialogs: onDialog(f) answers the next one with f,
  // accept() accepts it.  One listener for all, so that an answer that a
  // failed check left unused is replaced, never run twice; a dialog that no
  // check expects is dismissed
  let nextDialog = null;
  page.on('dialog', d => { const f = nextDialog; nextDialog = null; if (f) f(d); else d.dismiss(); });
  const onDialog = f => { nextDialog = f; };
  const accept = () => onDialog(d => d.accept());
  // another tab of the page, up to its first prompt
  async function tab() {
    const p = await t.context.newPage();
    t.watch(p);
    await p.goto(t.base + 'index.html');
    await p.waitForFunction(() => document.getElementById('status').dataset.state === 'waiting' &&
                            /(^|\n)st> $/.test(document.getElementById('term').textContent), null, { timeout: 90000 });
    return p;
  }
  async function evalIn(p, text, re, timeout = 30000) {
    const at = (await p.$eval('#term', e => e.textContent)).length;
    await p.fill('#line', text);
    await p.press('#line', 'Enter');
    await p.waitForFunction(([src, at]) => new RegExp(src).test(document.getElementById('term').textContent.slice(at)) &&
                            document.getElementById('status').dataset.state === 'waiting', [re.source, at], { timeout });
  }
  const saveAndWait = async () => {
    await page.evaluate(() => { document.getElementById('notice-text').textContent = ''; });
    await page.click('#save');
    await page.waitForFunction(() => /saved in this browser/.test(document.getElementById('notice-text').textContent) &&
                               !document.getElementById('notice').hidden, null, { timeout: 60000 });
  };

  // What the workers warn of (vm-worker.js passes on what the runtime and the
  // driver say, onDiag, as warnings) must not be an engine error or a
  // callback that threw, but while a check expects a crash
  const ENGINE_ERRORS = /RuntimeError|RangeError|Aborted|unreachable|signature_mismatch|unsupported syscall|^vm-driver: /;
  const diag = [];
  let crashing = false;
  const listen = p => p.on('worker', w => w.on('console', m => {
    if (!crashing && ENGINE_ERRORS.test(m.text())) diag.push(m.text());
  }));
  listen(page);
  t.context.on('page', listen);
  // the notice, with its buttons, once it says what re matches
  const noticeSays = (p, re, timeout = 90000) =>
    p.waitForFunction(src => !document.getElementById('notice').hidden &&
                      new RegExp(src).test(document.getElementById('notice-text').textContent), re.source, { timeout });
  const buttons = p => p.evaluate(() => ['notice-action', 'notice-alt'].map(id => document.getElementById(id))
                                    .filter(b => !b.hidden).map(b => b.textContent));

  // the texts of the status pill, as they come
  await t.context.addInitScript(() => {
    window.statusLog = [];
    document.addEventListener('DOMContentLoaded', () => {
      const el = document.getElementById('status-text');
      if (el) new MutationObserver(() => window.statusLog.push(el.textContent))
        .observe(el, { childList: true, characterData: true, subtree: true });
    });
  });

  await check('the page loads, without cross-origin isolation or SharedArrayBuffer', async () => {
    await page.goto(t.base + 'index.html');
    assert(await page.title() === 'Pharo Console', 'title');
    const [isolated, sab] = await page.evaluate(() => [self.crossOriginIsolated, typeof SharedArrayBuffer]);
    assert(isolated === false && sab === 'undefined', `crossOriginIsolated ${isolated}, SharedArrayBuffer ${sab}`);
  });

  await check('progress reaches 100%, then the prompt appears', async () => {
    const t0 = Date.now();
    await started();
    const log = await page.evaluate(() => window.statusLog);
    const full = log.indexOf('Loading 100%'), starting = log.indexOf('Starting'), waiting = log.indexOf('Waiting for input');
    assert(full >= 0 && starting > full && waiting > starting, 'status ' + JSON.stringify(log.slice(0, 8)) + ' ... ' +
           JSON.stringify(log.slice(-4)));
    assert(log.filter(s => /^Loading \d+%$/.test(s)).length >= 5, 'progress shown');
    assert(/^st> $/.test(await term()), 'only the prompt: ' + JSON.stringify(await term()));
    console.log('  # prompt after ' + (Date.now() - t0) + ' ms');
  });

  await check('3+4 and Enter gives 7; Transcript output', async () => {
    await evalTo('3+4', /\n7\nst> $/);
    assert(/^st> 3\+4\n7\n/.test(await term()), 'input echoed after the prompt');
    await evalTo("Transcript show: 'hello'; cr. #shown", /hello\n#shown\nst> $/);
  });

  await check('multi-line input with Shift+Enter', async () => {
    await mark();
    await page.fill('#line', '| a |');
    await page.press('#line', 'Shift+Enter');
    await page.keyboard.type('a := 6 * 7.');
    await page.press('#line', 'Shift+Enter');
    await page.keyboard.type('a');
    assert((await page.inputValue('#line')).split('\n').length === 3, 'newlines inserted');
    await page.press('#line', 'Enter');
    await waitSince(/\n42\nst> $/);
  });

  await check('history: ArrowUp recalls the last input', async () => {
    await page.focus('#line');
    await page.press('#line', 'ArrowUp');
    assert(await page.inputValue('#line') === '| a |\na := 6 * 7.\na', 'got ' + JSON.stringify(await page.inputValue('#line')));
    await page.press('#line', 'ArrowDown');
    assert(await page.inputValue('#line') === '', 'back to the empty draft');
  });

  await check('errors go to stderr, styled', async () => {
    await evalTo('nil foo', /MessageNotUnderstood[\s\S]*st> $/);
    assert(await page.$eval('#term', e => [...e.querySelectorAll('.err')].some(s => /^Error: MessageNotUnderstood/.test(s.textContent))),
           'error text in an .err span');
  });

  await check('Stop during an endless loop prints Interrupted.', async () => {
    await enter('[true] whileTrue');
    await waitStatus('busy');
    await page.waitForTimeout(300);
    const t1 = Date.now();
    await page.click('#stop');
    await waitSince(/Interrupted\.\n/, 5000);
    await waitStatus('waiting');
    console.log('  # stopped in ' + (Date.now() - t1) + ' ms');
    await evalTo('3 + 4', /\n7\nst> $/);
  });

  await check('Esc stops too', async () => {
    await enter('1 to: SmallInteger maxVal do: [ :i | ]');
    await waitStatus('busy');
    await page.press('#line', 'Escape');
    await waitSince(/Interrupted\.\n/, 5000);
    await waitStatus('waiting');
  });

  await check('Stop in a long primitive that never yields restarts the worker', async () => {
    // findString: in a long string is one primitive, without a single check
    // for interrupts: no slices end, no messages come, and the 3 s watchdog
    // replaces the worker
    await enter("(String new: 1000000 withAll: $a) findString: (String new: 8000 withAll: $a), 'b' startingAt: 1");
    await waitBusy();
    await page.waitForTimeout(500);
    const t1 = Date.now();
    await page.click('#stop');
    await waitSince(/VM restarted \(unsaved changes lost\)"\n[\s\S]*st> $/, 90000);
    await waitStatus('waiting');
    const ms = Date.now() - t1;
    console.log('  # replaced in ' + ms + ' ms');
    assert(ms >= 2900, 'not before the 3 s watchdog');
    await evalTo('5 + 5', /\n10\nst> $/);
  });

  await check('a Delay shows Sleeping, then the result', async () => {
    await enter('(Delay forSeconds: 2) wait. #woke');
    await waitStatus('sleeping', 5000);
    await waitSince(/#woke\nst> $/, 10000);
    await waitStatus('waiting');
  });

  await check('Save, reload: the image comes from IndexedDB, with its marker', async () => {
    await evalTo('Smalltalk at: #PageMarker put: 777', /\n777\nst> $/);
    await mark();
    await saveAndWait();
    // the REPL answers in a slice after the one that saved
    await waitSince(/\na SnapshotOperation\nst> $/);
    assert(!(await page.isDisabled('#reset')), 'Reset enabled');
    const n = t.requests.length;
    await reload();
    const served = requestsSince(n);
    assert(!served.includes(imageUrl) && !served.includes(changesUrl), 'not fetched again: ' + served.join(' '));
    assert(served.includes(sourcesUrl), 'the .sources is fetched');
    assert(/Started the image saved in this browser/.test(await term()), 'note: ' + JSON.stringify(await term()));
    await evalTo('Smalltalk at: #PageMarker', /\n777\nst> $/);
  });

  await check('Esc while Starting (before the first prompt) restarts nothing', async () => {
    const p = await t.context.newPage();
    t.watch(p);
    try {
      await p.goto(t.base + 'index.html');
      // Esc as soon as the page says Starting
      await p.waitForFunction(() => {
        if (document.getElementById('status').dataset.state !== 'starting') return false;
        document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
        return true;
      }, null, { timeout: 90000, polling: 5 });
      await p.waitForFunction(() => document.getElementById('status').dataset.state === 'waiting', null, { timeout: 90000 });
      await p.waitForTimeout(3500);                 // past the watchdog
      const text = await p.$eval('#term', e => e.textContent);
      assert(!/VM restarted/.test(text) && /(^|\n)st> $/.test(text), 'term ' + JSON.stringify(text));
    } finally { await p.close(); }
  });

  await check('two tabs: once one saves, the other says so and stores no .changes over it', async () => {
    // the image of the slot reads its sources at offsets of its own .changes
    const b = await tab();
    try {
      assert(/Started the image saved in this browser/.test(await b.$eval('#term', e => e.textContent)), 'b from the slot');
      await evalTo("Object compile: 'tabProbeP ^ ''the source of this tab''' classified: 'spec'. #p", /\n#p\nst> $/);
      await saveAndWait();
      await waitStatus('waiting');
      await evalIn(b, "Object compile: 'tabProbeQ ^ ''the other tab wrote this, at the same offset''' classified: 'spec'. #q",
                   /\n#q\nst> $/);
      await b.waitForFunction(() => /Another tab has replaced/.test(document.getElementById('notice-text').textContent) &&
                              !document.getElementById('notice').hidden, null, { timeout: 15000 });
      assert(await b.textContent('#notice-action') === 'Save', 'b offers Save');
    } finally { await b.close(); }
    await reload();
    await evalTo('(Object >> #tabProbeP) sourceCode', /\nst> $/);
    assert(/'tabProbeP \^ ''the source of this tab'''\nst> $/.test(await since()), 'the source ' + JSON.stringify(await since()));
  });

  let downloaded = null;
  await check('Download gives the image (at least 50 MB) and its .changes', async () => {
    const files = [];
    const got = d => files.push(d);
    page.on('download', got);
    try {
      await page.click('#download');
      const t1 = Date.now();
      while (files.length < 2 && Date.now() - t1 < 30000) await page.waitForTimeout(100);
    } finally { page.off('download', got); }
    assert(files.length === 2, files.length + ' downloads');
    const byName = {};
    for (const d of files) {
      byName[d.suggestedFilename()] = path.join(tmp, t.name + '-' + d.suggestedFilename());
      await d.saveAs(byName[d.suggestedFilename()]);
    }
    const image = byName['Pharo.image'], changes = byName['Pharo.changes'];
    assert(image && changes, 'names ' + Object.keys(byName));
    assert(fs.statSync(image).size >= 50e6, 'image of ' + fs.statSync(image).size + ' bytes');
    assert(fs.statSync(changes).size > 0, 'a .changes');
    downloaded = { image, changes };
  });

  await check('Reset deletes the saved image and starts the original one', async () => {
    const n = t.requests.length;
    await mark();
    accept();
    await page.click('#reset');
    await waitSince(/Deleted the image saved in this browser[\s\S]*st> $/, 90000);
    await waitStatus('waiting');
    assert(requestsSince(n).includes(imageUrl), 'the image is fetched again');
    assert(await page.isDisabled('#reset'), 'nothing to reset');
    await evalTo('Smalltalk at: #PageMarker ifAbsent: [ #none ]', /\n#none\nst> $/);
  });

  await check('Upload starts the downloaded image and keeps it once it prompts, also across a reload', async () => {
    assert(downloaded, 'nothing downloaded');
    await mark();
    await page.setInputFiles('#upload', [downloaded.image, downloaded.changes]);
    await waitSince(/Started the uploaded image\."\n[\s\S]*st> $/, 90000);
    await waitStatus('waiting');
    await noticeSays(page, /The uploaded image is now kept in this browser/, 30000);
    assert(!(await page.isDisabled('#reset')), 'Reset enabled');
    await evalTo('Smalltalk at: #PageMarker', /\n777\nst> $/);
    const n = t.requests.length;
    await reload();
    assert(!requestsSince(n).includes(imageUrl), 'restored, not fetched');
    await evalTo('Smalltalk at: #PageMarker', /\n777\nst> $/);
  });

  await check('an upload that does not start is not kept: Restart starts the saved image', async () => {
    assert(downloaded, 'nothing downloaded');
    const broken = path.join(tmp, t.name + '-broken.image');
    fs.writeFileSync(broken, fs.readFileSync(downloaded.image).subarray(0, 1 << 20));
    crashing = true;
    try {
      await mark();
      accept();                           // replace the saved image
      await page.setInputFiles('#upload', [broken]);
      await waitStatus('crashed', 90000);
      await noticeSays(page, /^The uploaded image did not start; it was not kept\.$/, 5000);
      assert(JSON.stringify(await buttons(page)) === '["Restart"]', 'buttons ' + JSON.stringify(await buttons(page)));
      // the saved image is still there: Reset may delete it, and another
      // upload asks before it replaces it
      assert(!(await page.isDisabled('#reset')), 'Reset is disabled');
      let asked = null;
      onDialog(d => { asked = d.message(); d.dismiss(); });
      await page.setInputFiles('#upload', [broken]);
      for (let i = 0; i < 50 && asked === null; i++) await new Promise(r => setTimeout(r, 100));
      assert(/^Replace the image saved in this browser with /.test(asked || ''), 'asked ' + JSON.stringify(asked));
      assert(await page.$eval('#status', e => e.dataset.state) === 'crashed', 'a dismissed upload starts nothing');
      await mark();
      await page.click('#notice-action');
      await waitSince(/Started the image saved in this browser[\s\S]*st> $/, 90000);
      await waitStatus('waiting');
    } finally { crashing = false; }
    await evalTo('Smalltalk at: #PageMarker', /\n777\nst> $/);
  });

  await check('a saved image that does not start: the notice offers Reset next to Restart', async () => {
    // the image of the slot, cut short, as by a broken save
    await page.evaluate(async () => {
      const db = await new Promise((resolve, reject) => {
        const r = indexedDB.open('pharo-wasm', 1);
        r.onsuccess = () => resolve(r.result);
        r.onerror = () => reject(r.error);
      });
      const blob = await new Promise(resolve => {
        const r = db.transaction('files').objectStore('files').get('Pharo.image');
        r.onsuccess = () => resolve(r.result);
      });
      // a Blob of its own: Firefox stores a slice of a stored Blob whole
      const cut = new Blob([await blob.slice(0, 1 << 20).arrayBuffer()]);
      await new Promise((resolve, reject) => {
        const tx = db.transaction('files', 'readwrite');
        tx.objectStore('files').put(cut, 'Pharo.image');
        tx.oncomplete = resolve;
        tx.onabort = () => reject(tx.error);
      });
      db.close();
    });
    crashing = true;
    try {
      markAt = 0;
      await page.reload();
      await page.waitForFunction(() => /^(crashed|exited)$/.test(document.getElementById('status').dataset.state),
                                 null, { timeout: 90000 });
      await noticeSays(page, /^The image saved in this browser did not start\.$/, 5000);
      assert(JSON.stringify(await buttons(page)) === '["Restart","Reset saved image"]',
             'buttons ' + JSON.stringify(await buttons(page)));
      await mark();
      accept();
      await page.click('#notice-alt');
      await waitSince(/Deleted the image saved in this browser[\s\S]*st> $/, 90000);
      await waitStatus('waiting');
    } finally { crashing = false; }
    assert(await page.isDisabled('#reset'), 'nothing to reset');
    await evalTo('Smalltalk at: #PageMarker ifAbsent: [ #none ]', /\n#none\nst> $/);
  });

  if (t.shots) {
    await page.click('#clear');
    await evalTo('3 + 4', /\n7\nst> $/);
    await evalTo('100 factorial printString size', /\n158\nst> $/);
    await evalTo("'Hello from WebAssembly' reversed", /\nst> $/);
    await evalTo('(1 to: 10) collect: [ :i | i * i ]', /\nst> $/);
    await evalTo('Smalltalk vm architectureName', /'wasm64'\nst> $/);
    await evalTo('1/0', /ZeroDivide[\s\S]*st> $/);
    await page.fill('#line', '(1 to: 5) inject: 0 into: [ :a :b | a + b ]');
    if (await page.isVisible('#notice')) await page.click('#notice-close');
    await page.mouse.move(0, 0);
    for (const scheme of ['light', 'dark']) {
      await page.emulateMedia({ colorScheme: scheme });
      await page.setViewportSize({ width: 1280, height: 800 });
      await page.waitForTimeout(400);             // let colour transitions finish
      await page.screenshot({ path: t.shot(`console-${scheme}-1280.png`) });
      await page.setViewportSize({ width: 360, height: 740 });
      await page.waitForTimeout(200);
      await page.screenshot({ path: t.shot(`console-${scheme}-360.png`) });
    }
    await page.fill('#line', '');
    await page.emulateMedia({ colorScheme: 'light' });
    await page.setViewportSize({ width: 1280, height: 800 });
  }

  await check('dark and light: prefers-color-scheme, and the theme button over it', async () => {
    const bg = () => page.evaluate(() => getComputedStyle(document.body).backgroundColor);
    await page.emulateMedia({ colorScheme: 'light' });
    const light = await bg();
    await page.emulateMedia({ colorScheme: 'dark' });
    const dark = await bg();
    assert(light !== dark && light !== 'rgba(0, 0, 0, 0)' && dark !== 'rgba(0, 0, 0, 0)', `backgrounds ${light} ${dark}`);
    await page.click('#theme');                   // light, whatever the system says
    assert(await bg() === light && await page.textContent('#theme-label') === 'Light', 'theme light');
    await page.click('#theme');
    await page.emulateMedia({ colorScheme: 'light' });
    assert(await bg() === dark && await page.textContent('#theme-label') === 'Dark', 'theme dark');
    await page.click('#theme');
    assert(await bg() === light && await page.textContent('#theme-label') === 'System', 'theme system');
  });

  await check('Clear and Ctrl+L keep the prompt', async () => {
    const onlyPrompt = () => page.waitForFunction(() => document.getElementById('term').textContent === 'st> ',
                                                  null, { timeout: 5000 });
    await page.click('#clear');
    await onlyPrompt();
    await evalTo('2 + 2', /\n4\nst> $/);
    assert(await term() === 'st> 2 + 2\n4\nst> ', 'the input after the prompt: ' + JSON.stringify(await term()));
    await page.press('#line', 'Control+l');
    await onlyPrompt();
  });

  await check('long output streams and stays capped', async () => {
    await page.click('#clear');
    await enter('1 to: 30000 do: [ :i | Transcript show: i printString; cr ]. #done');
    await waitSince(/\n29999\n30000\n#done\nst> $/, 120000);
    await waitStatus('waiting');
    const n = await page.$eval('#term', e => e.textContent.split('\n').length);
    assert(n <= 20001, 'terminal capped at 20000 lines, has ' + n);
    await page.click('#clear');
    await evalTo('1 + 1', /\n2\nst> $/);
  });

  await check('more than 1 MiB of output: the page acks what it rendered, and the VM goes on to the end', async () => {
    // the worker pauses the VM above 1 MiB of output not acked: without the
    // acks of the page the Console would stall for good
    await page.click('#clear');
    const t1 = Date.now();
    await enter("1 to: 20000 do: [ :i | Transcript show: (i printPaddedWith: $0 to: 10) , (String new: 89 withAll: $x); cr ]. #done");
    await waitSince(/\n0000019999x{89}\n0000020000x{89}\n#done\nst> $/, 90000);
    await waitStatus('waiting');
    console.log('  # 2 MB in ' + (Date.now() - t1) + ' ms');
    const [n, c] = await page.$eval('#term', e => [e.textContent.split('\n').length, e.textContent.length]);
    assert(n <= 20001 && c <= 2000000, `terminal capped at 20000 lines and 2 MB, has ${n} lines, ${c} characters`);
    await page.click('#clear');
    await evalTo('1 + 1', /\n2\nst> $/);
  });

  await check('Clear while 1 MiB of output waits to be rendered acks it, and the VM goes on to the end', async () => {
    // The page's frames are held, so that the output piles up unrendered,
    // and so unacked, until the worker pauses the VM at 1 MiB.  Clear then
    // drops it: unless it acks what it dropped, the worker never resumes the
    // VM.  The page's posts to its worker and what the worker sends are
    // counted on the way (its prototype, which the page's worker uses).
    await page.click('#clear');
    await page.waitForFunction(() => document.getElementById('term').textContent === 'st> ', null, { timeout: 5000 });
    await page.evaluate(() => {
      const T = window.clearCheck = { frames: [], raf: window.requestAnimationFrame, post: Worker.prototype.postMessage,
                                      out: 0, acked: 0, counting: false };
      window.requestAnimationFrame = f => { T.frames.push(f); return -1; };
      Worker.prototype.postMessage = function (m, ...rest) {
        if (!T.counting) {
          T.counting = true;
          this.addEventListener('message', e => { if (e.data && e.data.type === 'output') T.out += e.data.text.length; });
        }
        if (m && m.type === 'ack') T.acked += m.chars;
        return T.post.call(this, m, ...rest);
      };
    });
    const counts = () => page.evaluate(() => ({ out: window.clearCheck.out, acked: window.clearCheck.acked }));
    const release = () => page.evaluate(() => {
      const T = window.clearCheck;
      window.requestAnimationFrame = T.raf;
      Worker.prototype.postMessage = T.post;
      for (const f of T.frames.splice(0)) f(performance.now());
    });
    try {
      await enter("1 to: 20000 do: [ :i | Transcript show: (i printPaddedWith: $0 to: 10) , (String new: 89 withAll: $x); cr ]. #done");
      // (polled from here: waitForFunction would wait for a frame)
      let c = await counts(), still = 0;
      for (const t0 = Date.now(); still < 4 && Date.now() - t0 < 60000; ) {
        await new Promise(r => setTimeout(r, 250));
        const d = await counts();
        still = d.out >= 1 << 20 && d.out === c.out ? still + 1 : 0;
        c = d;
      }
      assert(still >= 4 && c.acked === 0 && c.out < 20000 * 100,
             `the VM paused at 1 MiB, with nothing acked: ${c.out} characters sent, ${c.acked} acked`);
      const after = await page.evaluate(() => {
        const T = window.clearCheck, out = T.out;
        document.getElementById('clear').click();
        return { out, acked: T.acked };
      });
      assert(after.acked === after.out, `Clear acked ${after.acked} of the ${after.out} characters it dropped`);
    } finally {
      await release();
    }
    await waitSince(/\n0000019999x{89}\n0000020000x{89}\n#done\nst> $/, 90000);
    await waitStatus('waiting');
    await page.click('#clear');
    await evalTo('1 + 1', /\n2\nst> $/);
  });

  for (const width of [360, 320]) await check(`no horizontal scroll at ${width} px, 16 px gutters`, async () => {
    await page.setViewportSize({ width, height: 740 });
    await evalTo("String new: 300 withAll: $x", /xxxxxxxxxx'\nst> $/);
    const o = await page.evaluate(() => {
      const d = document.documentElement;
      const wide = [...document.querySelectorAll('body *')].filter(e => {
        const r = e.getBoundingClientRect();
        return r.width && (r.right > d.clientWidth + 0.5 || r.left < -0.5) && getComputedStyle(e).visibility !== 'hidden';
      }).map(e => e.tagName + (e.id ? '#' + e.id : ''));
      return { sw: d.scrollWidth, cw: d.clientWidth, bw: document.body.scrollWidth, wide: wide.slice(0, 5) };
    });
    assert(o.sw <= o.cw && o.bw <= o.cw && !o.wide.length, JSON.stringify(o));
    const gutter = await page.$eval('#console', e => [e.getBoundingClientRect().left,
                                                      document.documentElement.clientWidth - e.getBoundingClientRect().right]);
    assert(gutter.every(g => g >= 15.5 && g <= 16.5), '16 px gutters, got ' + gutter);
    await page.setViewportSize({ width: 1280, height: 800 });
  });

  await check('Restart gives a fresh prompt', async () => {
    await mark();
    await page.click('#restart');
    await waitSince(/Restarting…"\n[\s\S]*st> $/, 90000);
    await waitStatus('waiting');
    await evalTo('3 * 3', /\n9\nst> $/);
  });

  await check('Smalltalk exit: 7 shows the exit and offers Restart; Download still works', async () => {
    await enter('Smalltalk exit: 7');
    await waitStatus('exited');
    await waitSince(/Pharo quit \(exit code 7\)/);
    assert(await page.textContent('#status-text') === 'Exited (7)', 'status');
    assert(await page.isVisible('#notice-action') && await page.textContent('#notice-action') === 'Restart', 'Restart offered');
    assert(await page.isDisabled('#line') && await page.isDisabled('#stop') && await page.isDisabled('#save'), 'input disabled');
    assert(!(await page.isDisabled('#download')), 'Download enabled');
    await mark();
    await page.click('#notice-action');
    await started();
    assert(!(await page.isVisible('#notice')), 'notice hidden again');
  });

  await check('Ctrl+D asks, then ends the session', async () => {
    let asked = '';
    onDialog(d => { asked = d.message(); d.accept(); });
    await mark();
    await page.focus('#line');
    await page.press('#line', 'Control+d');
    await waitStatus('exited');
    assert(/session ended/.test(await page.textContent('#notice-text')), 'notice');
    assert(/^End the session\?/.test(asked), 'confirmation asked: ' + JSON.stringify(asked));
    await page.click('#notice-action');
    await started();
  });

  await check('a database that cannot be opened (a private window): a note, Save offers Download', async () => {
    // a fresh context whose workers find an IndexedDB that refuses to open
    const context = await t.browser.newContext();
    try {
      await context.route('**/vm-storage.js*', async route => {
        const body = fs.readFileSync(path.join(t.webDir, 'vm-storage.js'), 'utf8');
        route.fulfill({ contentType: 'text/javascript', body: `Object.defineProperty(self, 'indexedDB', { configurable: true, value: {
          open() {
            const r = {};
            setTimeout(() => { r.error = new DOMException('The operation is insecure.', 'SecurityError'); r.onerror(); });
            return r;
          } } });\n` + body });
      });
      const p = await context.newPage();
      t.watch(p);
      listen(p);
      await p.goto(t.base + 'index.html');
      await p.waitForFunction(() => document.getElementById('status').dataset.state === 'waiting', null, { timeout: 90000 });
      const text = await p.$eval('#term', e => e.textContent);
      assert(/"Note: this browser does not keep images here \(The operation is insecure\.\); use Download to keep a copy\."/.test(text),
             'note ' + JSON.stringify(text));
      await p.click('#save');
      await p.waitForFunction(() => !document.getElementById('notice').hidden &&
                              /^Saved, but/.test(document.getElementById('notice-text').textContent), null, { timeout: 60000 });
      assert(/use Download to keep a copy/.test(await p.textContent('#notice-text')) &&
             await p.textContent('#notice-action') === 'Download', 'notice ' + await p.textContent('#notice-text'));
      assert(await p.isDisabled('#reset'), 'nothing to reset');
      await p.waitForFunction(() => document.getElementById('status').dataset.state === 'waiting', null, { timeout: 30000 });
      await evalIn(p, '3 + 4', /\n7\nst> $/);
    } finally { await context.close(); }
  });

  await check('from file:// the page explains that it needs HTTP', async () => {
    const f = await t.context.newPage();
    t.watch(f);
    await f.goto('file://' + path.join(t.webDir, 'index.html'));
    await f.waitForFunction(() => document.getElementById('status').dataset.state === 'error', null, { timeout: 10000 });
    assert(/served over HTTP/.test(await f.textContent('#notice-text')), 'notice: ' + await f.textContent('#notice-text'));
    assert(await f.isDisabled('#line'), 'input disabled');
    await f.close();
  });

  await check('no worker warned of an engine error or of a callback that threw', async () => {
    assert(!diag.length, diag.slice(0, 5).join(' | '));
  });
});
