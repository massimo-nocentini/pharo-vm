// world.spec.mjs - browser test of the world page (build-wasm/web/world.html)
//
// usage: node world.spec.mjs WEB_DIR
//
// A plain node script, run by `make wasm-check-browser' and never a build
// dependency.  tests/wasm/lib/pw.mjs serves WEB_DIR (no COOP/COEP headers)
// and drives the page in each browser named by BROWSERS (default
// "chromium"); see there for PLAYWRIGHT_MODULE and SHOTS, which saves light
// and dark screenshots at 1280 and 360 px.  Each browser gets a context of
// its own, so its IndexedDB starts empty.  WEB_DIR must have the world
// image (manifest.json world: true).
//
// The world boots with tests/wasm/st/world-probe.st as well: the spec serves
// a vm-driver.js that adds it to the arguments and the files of the world.
// The probe writes /pharo/probe.json, which the spec reads through the
// page's PharoWorld.readFile, so it knows where the menus and windows are,
// what the Playground says and whether a debugger opened; the pixels of the
// canvas say that the world was painted, and the probe which fonts draw it:
// those of manifest.fonts, FreeType (with glyphs for a lambda and an arrow)
// or bitmap fonts, in the world image and in a stock image prepared by the
// page.  With cairo (manifest.json lists libcairo.so.2 in its libraries),
// the probe opens the windows that draw with Athens on request, when the
// spec writes their name into /pharo/probe-scene.txt: an Inspector on a
// Roassal canvas, whose Canvas view must show the canvas's green box, Code
// Changes (Epicea, whose graph Hiedra draws) and, on Pharo 12, the color
// picker, each without a morph that failed to draw, an Inspector that says
// 'Error while creating the inspector', or a debugger.  The spec drives the
// page with real pointer and keyboard events,
// and prints the cold first-frame (a first visit: nothing cached, an empty
// IndexedDB) and keystroke-to-canvas latencies.  An init script keeps the
// input records that world.js posts to the worker, for the checks of the
// wheel, the focus and the buttons, and the states of the status pill, the
// overlay and the notice.  Exits with status 1 if any check fails, the page
// logs an error, or a worker warns of an engine error or an exception of the
// display (what the emscripten runtime says goes to the console as
// warnings).
//
// Open is checked with the stock image of the build (WASM_DIR/image/stock),
// zipped by tests/wasm/lib/zip.mjs in a directory as files.pharo.org does,
// with a line added to its .sources (a .sources of its own): the page must
// prepare it for the world, then boot it, also from the image saved in
// this browser.  Then the world image of the Download check is dropped on
// the page, which boots it at once.

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { run, decodePNG, dropFiles } from './lib/pw.mjs';
import { imageZip } from './lib/zip.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const scratch = process.env.TEST_DIR || os.tmpdir();
fs.mkdirSync(scratch, { recursive: true });
const tmp = fs.mkdtempSync(path.join(scratch, 'world-spec-'));
process.on('exit', () => fs.rmSync(tmp, { recursive: true, force: true }));
const probeSource = fs.readFileSync(path.join(here, 'st', 'world-probe.st'), 'utf8');
// appended to vm-driver.js: the world boots with the probe
const probePatch = `
;(function () {
  // world.spec.mjs: boot the world with tests/wasm/st/world-probe.st
  var D = self.PharoVMDriver, start = D.start, vmArgs = D.vmArgs, probe = ${JSON.stringify(probeSource)};
  D.vmArgs = function (mode, image) {
    var args = vmArgs(mode, image);
    return mode === 'world' ? args.concat(['st', '/pharo/st/world-probe.st']) : args;
  };
  D.start = function (create, o) {
    o.files = (o.files || []).concat([{ path: '/pharo/st/world-probe.st', data: probe }]);
    return start(create, o);
  };
})();
`;

// What the warnings of the workers must not say
const ENGINE_ERRORS = /RuntimeError|RangeError|Aborted|unreachable|signature_mismatch|unsupported syscall|webDisplay\./;
// The record types of OSWebDriver
const PRESS = 2, RELEASE = 3, WHEEL = 4, KEY_DOWN = 5, KEY_UP = 6, FOCUS = 9;

// The stock image of the build, zipped as files.pharo.org does, with a
// .sources of its own; written once
const SOURCES_TAIL = '\r"world.spec.mjs: a .sources of its own"\r';
let stockZip = null;
function zipStock(webDir) {
  if (stockZip) return stockZip;
  const stock = path.join(webDir, '..', 'image', 'stock');
  const sources = fs.readdirSync(stock).find(f => f.endsWith('.sources'));
  stockZip = { path: path.join(tmp, 'stock12.zip'), name: 'Pharo12.0-SNAPSHOT-64bit-world.image',
               ownSize: fs.statSync(path.join(stock, sources)).size + SOURCES_TAIL.length };
  fs.writeFileSync(stockZip.path, imageZip(stock, { base: 'Pharo12.0-SNAPSHOT-64bit-world', folder: 'world', sourcesTail: SOURCES_TAIL }));
  return stockZip;
}

// What the probe says of the fonts that draw the world, against those of
// the build (manifest.fonts): '' when they are those, else what differs
function wrongFonts(p, fonts) {
  const freetype = fonts === 'freetype', wrong = [];
  if (!p.fonts) return 'the probe says no fonts';
  for (const which of ['default', 'code', 'menu', 'windowTitle', 'menubar']) {
    const cls = p.fonts[which];
    if (freetype ? cls !== 'FreeTypeFont' : !cls || cls === 'FreeTypeFont') wrong.push(which + ' ' + cls);
  }
  if (freetype && p.fonts.family !== 'Source Sans Pro') wrong.push('family ' + p.fonts.family);
  if (p.glyphs !== freetype) wrong.push('glyphs for \u03bb\u2192 ' + p.glyphs);
  return wrong.length ? `not the ${fonts} fonts of the build: ${wrong.join(', ')}` : '';
}
// The fonts that a preparation sets up, as world.js names them
const fontsNamed = fonts => fonts === 'freetype' ? 'FreeType' : 'bitmap';

const median = a => { const s = a.slice().sort((x, y) => x - y); return s.length ? s[Math.floor((s.length - 1) / 2)] : NaN; };
const center = b => [(b[0] + b[2]) / 2, (b[1] + b[3]) / 2];
const inside = (p, b) => p[0] >= b[0] && p[0] < b[2] && p[1] >= b[1] && p[1] < b[3];

await run(async t => {
  const { page, check, assert, manifest } = t;
  if (!manifest.world) throw new Error(t.webDir + '/manifest.json has no world image (world: false)');
  const imageUrl = '/' + manifest.files.find(f => f.path === manifest.image).url;
  const siteSources = manifest.files.find(f => f.path.endsWith('.sources'));
  const sourcesUrl = '/' + siteSources.url;

  await t.context.route('**/vm-driver.js*', async route => {
    const body = fs.readFileSync(path.join(t.webDir, 'vm-driver.js'), 'utf8');
    route.fulfill({ contentType: 'text/javascript', body: body + probePatch });
  });

  // world.js logs a crash of the VM, with the Smalltalk stacks, as a warning
  page.on('console', m => {
    if (/^pharo: /.test(m.text())) console.log('  # the VM crashed: ' + m.text().split('\n').slice(0, 12).join('\n  #   '));
  });
  // vm-worker.js passes on what the runtime says (onDiag) as warnings
  const diag = [];
  page.on('worker', w => w.on('console', m => { if (ENGINE_ERRORS.test(m.text())) diag.push(m.text()); }));

  // The input records that world.js posts, and the worker it posts them to;
  // and the states of the pill, the texts of the overlay and of the notice
  await page.addInitScript(() => {
    const post = Worker.prototype.postMessage;
    window.__records = [];
    Worker.prototype.postMessage = function (m, transfer) {
      window.__worker = this;
      if (m && m.type === 'display' && m.kind === 'event') window.__records.push(m.event);
      return post.call(this, m, transfer);
    };
    window.__log = { states: [], overlays: [], notices: [] };
    document.addEventListener('DOMContentLoaded', () => {
      const watch = (id, f, log) => new MutationObserver(() => log.push(f(document.getElementById(id))))
        .observe(document.getElementById(id), { attributes: true, childList: true, characterData: true, subtree: true });
      watch('status', e => e.dataset.state, window.__log.states);
      watch('overlay-text', e => e.textContent, window.__log.overlays);
      watch('notice-text', e => e.textContent, window.__log.notices);
    });
  });
  const clearLog = () => page.evaluate(() => { for (const k in window.__log) window.__log[k].length = 0; });
  const log = () => page.evaluate(() => window.__log);
  // the dialogs of f, answered with accept
  async function accepting(f) {
    const dialogs = [];
    const dialog = d => { dialogs.push(d.message() || d.type()); d.accept(); };
    page.on('dialog', dialog);
    try { await f(); } finally { page.off('dialog', dialog); }
    return dialogs;
  }
  const noticeSays = (re, timeout = 30000) =>
    page.waitForFunction(src => !document.getElementById('notice').hidden &&
                         new RegExp(src).test(document.getElementById('notice-text').textContent), re.source, { timeout });
  const records = () => page.evaluate(() => window.__records.splice(0));
  // the records of these types, as [type, a, mods & 0xFF, buttons, c, d]
  const kinds = (list, ...types) => list.filter(r => types.includes(r[0])).map(r => [r[0], r[4], r[5] & 0xff, r[5] >> 8, r[6], r[7]]);
  const sinkFocused = () => page.evaluate(() => document.activeElement === document.getElementById('sink'));
  // what the page does with a beforeunload, here a synthetic one
  const asksBeforeUnload = () => page.evaluate(() => {
    const e = new Event('beforeunload', { cancelable: true });
    dispatchEvent(e);
    return e.defaultPrevented;
  });

  const stats = () => page.evaluate(() => window.PharoWorld.stats);
  const waitState = (st, timeout = 30000) =>
    page.waitForFunction(s => window.PharoWorld && window.PharoWorld.state === s, st, { timeout });
  const frames = () => page.evaluate(() => window.PharoWorld.stats.frames);
  const canvasBox = () => page.locator('#world').boundingBox();
  const shoot = async () => decodePNG(await page.locator('#world').screenshot());
  // the canvas's pixels in a rectangle of the world, as a string of RGBA
  const region = (shot, b) => {
    const parts = [];
    for (let y = Math.max(0, b[1]); y < Math.min(shot.height, b[3]); y++)
      parts.push(shot.data.subarray((y * shot.width + Math.max(0, b[0])) * 4, (y * shot.width + Math.min(shot.width, b[2])) * 4).toString('hex'));
    return parts.join('');
  };

  // The probe written after the call that pred accepts
  async function probe(what, pred, timeout = 30000) {
    const t0 = Date.now();
    let first = null, last = null;
    for (;;) {
      let p = null;
      try {
        p = JSON.parse(await page.evaluate(() => window.PharoWorld.readFile('/pharo/probe.json')
          .then(d => new TextDecoder().decode(d))));
      } catch (e) { /* not yet, or being written */ }
      if (p && p.seq) {
        if (first === null) first = p.seq;
        last = p;
        if (p.seq > first && pred(p)) return p;
      }
      if (Date.now() - t0 > timeout) {
        const shown = await page.evaluate(() => [document.getElementById('status-text').textContent,
                                                 document.getElementById('notice-text').textContent]);
        throw new Error('timed out after ' + timeout + ' ms waiting for ' + what + '; the page says ' + JSON.stringify(shown) +
                        '; probe: ' + JSON.stringify(last).slice(0, 1200));
      }
      await page.waitForTimeout(100);
    }
  }
  // A click at a point of the world
  async function click(p, button = 'left') {
    const box = await canvasBox();
    await page.mouse.click(box.x + p[0], box.y + p[1], { button, delay: 30 });
  }
  const menuItem = (p, label) => {
    for (const m of p.menus) for (const i of m.items) if (i.label === label) return i;
    return null;
  };
  // How many pixels of the colour rgb ([r, g, b]) a screenshot has in a
  // rectangle of the world
  function pixelsOf(shot, b, rgb) {
    let n = 0;
    for (let y = Math.max(0, b[1]); y < Math.min(shot.height, b[3]); y++)
      for (let x = Math.max(0, b[0]); x < Math.min(shot.width, b[2]); x++) {
        const i = (y * shot.width + x) * 4;
        if (shot.data[i] === rgb[0] && shot.data[i + 1] === rgb[1] && shot.data[i + 2] === rgb[2]) n++;
      }
    return n;
  }
  // What is wrong with the windows that a scene of the probe opened, or ''
  function sceneWrong(p) {
    const wrong = [];
    if (p.scene.error) wrong.push('it raised ' + p.scene.error);
    if (p.debugger) wrong.push('a debugger opened');
    for (const w of p.scene.windows) {
      if (w.error) wrong.push(`${w.label}: the probe raised ${w.error}`);
      if (w.inspectorError) wrong.push(`${w.label}: 'Error while creating the inspector'`);
      if (w.failing && w.failing.length) wrong.push(`${w.label}: morphs that failed to draw: ${w.failing.join(' ')}`);
    }
    return wrong.join('; ');
  }
  // A point of the desktop, right of every window
  const desktop = p => {
    const x = p.world[0] - 30, y = Math.round(p.world[1] / 2);
    assert(!p.windows.some(w => inside([x, y], w.bounds)), 'a free point of the desktop');
    return [x, y];
  };
  // A paste, as a browser makes it: Ctrl+V, then the paste event with the
  // text, here 100 ms later, so that the world runs in between: the page
  // must hold the key back until the text went.  (Firefox hides the data
  // of a DataTransfer given to a synthetic event, not of one the event has.)
  const paste = text => page.evaluate(async text => {
    const sink = document.getElementById('sink');
    const key = type => sink.dispatchEvent(new KeyboardEvent(type, { key: 'v', code: 'KeyV', ctrlKey: true, bubbles: true, cancelable: true }));
    key('keydown');
    await new Promise(r => setTimeout(r, 100));
    const data = new DataTransfer(), event = new ClipboardEvent('paste', { bubbles: true, cancelable: true });
    data.setData('text/plain', text);
    Object.defineProperty(event, 'clipboardData', { value: data });
    sink.dispatchEvent(event);
    key('keyup');
  }, text);

  // Browse > Playground: a new Playground, in front
  async function openPlayground() {
    p = await probe('the world', p => p.menubar.length);
    const n = p.windows.filter(w => w.label === 'Playground').length;
    // Pharo 15 under Firefox sometimes ignores the first click after the
    // focus checks above, which leave pressed buttons and Shift released by
    // the page: click again, at most twice, and say so
    for (let tries = 1; ; tries++) {
      await click(center(p.menubar.find(i => i.label === 'Browse').bounds));
      try {
        p = await probe('the Browse menu', p => menuItem(p, 'Playground'), tries < 3 ? 5000 : 30000);
        break;
      } catch (e) {
        if (tries === 3) throw e;
        console.log(`  # the Browse menu did not open after click ${tries}: click again`);
      }
    }
    await click(center(menuItem(p, 'Playground').bounds));
    p = await probe('a new Playground', p => p.playground && p.menus.length === 0 &&
                    p.windows.filter(w => w.label === 'Playground').length > n);
  }
  // Ctrl+D on code pasted into the Playground in front
  async function doIt(code) {
    await click(center(p.playground.bounds));
    await page.keyboard.press('Control+a');
    await paste(code);
    p = await probe('the Playground saying ' + code, p => p.playground.text === code);
    await page.keyboard.press('Control+d');
  }

  let p, t0;
  await check('the page loads, without cross-origin isolation or SharedArrayBuffer', async () => {
    t0 = Date.now();
    await page.goto(t.base + 'world.html');
    assert(await page.title() === 'Pharo World', 'title ' + await page.title());
    const [isolated, sab] = await page.evaluate(() => [self.crossOriginIsolated, typeof SharedArrayBuffer]);
    assert(isolated === false && sab === 'undefined', `crossOriginIsolated ${isolated}, SharedArrayBuffer ${sab}`);
  });

  await check('the world paints within 60 s', async () => {
    await page.waitForFunction(() => window.PharoWorld.stats.frames > 0, null, { timeout: 60000 });
    const s = await stats();
    console.log(`  # cold first frame ${s.firstFrameAt.toFixed(0)} ms after the navigation (${Date.now() - t0} ms here; ` +
                `nothing cached, an empty IndexedDB, ${manifest.fonts} fonts)`);
    await waitState('running');
    assert(await page.isHidden('#overlay'), 'the loading card is gone');
    assert(/^Pharo - /.test(await page.title()), 'the title of the image: ' + await page.title());
    const box = await canvasBox();
    assert(String(s.firstFrameSize) === [Math.round(box.width), Math.round(box.height)].join(','),
           `the first frame ${s.firstFrameSize} has the size of the canvas ${box.width}x${box.height}`);
    const shot = await shoot(), counts = new Map();
    for (let i = 0; i < shot.data.length; i += 4) {
      const k = shot.data.readUInt32LE(i);
      counts.set(k, (counts.get(k) || 0) + 1);
    }
    const top = Math.max(...counts.values()) / (shot.width * shot.height);
    assert(counts.size > 64 && top < 0.9, `a non-uniform canvas: ${counts.size} colours, the commonest ${(100 * top).toFixed(1)}%`);
  });

  await check('the world has the size of the canvas, one pixel per CSS pixel', async () => {
    const box = await canvasBox();
    p = await probe('a World of the canvas size', p => p.world[0] === Math.round(box.width) && p.world[1] === Math.round(box.height));
    assert(p.driver === 'OSWebDriver', 'driver ' + p.driver);
    const bitmap = await page.$eval('#world', c => [c.width, c.height]);
    assert(bitmap[0] === p.world[0] && bitmap[1] === p.world[1], 'the canvas bitmap ' + bitmap);
  });

  await check(`the world draws its text with the fonts of the build: ${manifest.fonts === 'freetype' ? 'FreeType, which has glyphs for \u03bb\u2192' : 'bitmap fonts'}`, async () => {
    assert(manifest.fonts === 'freetype' || manifest.fonts === 'bitmap', 'manifest.fonts ' + manifest.fonts);
    p = await probe('the fonts of the world', p => p.fonts);
    const wrong = wrongFonts(p, manifest.fonts);
    assert(!wrong, wrong);
  });

  await check('a right click on the desktop opens the world menu; Escape closes it', async () => {
    const at = desktop(p), before = await shoot();
    await click(at, 'right');
    try {
      p = await probe('the world menu', p => p.menus.length > 0);
      const items = p.menus[0].items.map(i => i.bounds);
      const bounds = items.reduce((b, i) => [Math.min(b[0], i[0]), Math.min(b[1], i[1]), Math.max(b[2], i[2]), Math.max(b[3], i[3])]);
      // the probe runs every 100 ms, so the frames of the menu came by now
      await page.waitForTimeout(200);
      assert(region(await shoot(), bounds) !== region(before, bounds), 'the canvas shows the menu at ' + bounds);
    } finally {
      await page.keyboard.press('Escape');
    }
    p = await probe('the menu closed', p => p.menus.length === 0);
  });

  await check('the wheel sends notches, positive away from the user, where the pointer is', async () => {
    const at = desktop(p), box = await canvasBox();
    await page.mouse.move(box.x + at[0], box.y + at[1]);
    await records();
    await page.mouse.wheel(0, 120);
    await page.waitForFunction(() => window.__records.some(r => r[0] === 4));
    const wheel = (await records()).filter(r => r[0] === WHEEL);
    assert(wheel.length === 1 && Math.abs(wheel[0][2] - at[0]) <= 1 && Math.abs(wheel[0][3] - at[1]) <= 1 &&
           wheel[0][6] === 0 && wheel[0][7] === -3, 'one record [4, t, x, y, 0, mods, 0, -3]: ' + JSON.stringify(wheel));
  });

  await check('a second button pressed while one is held keeps the focus, and both are pressed and released', async () => {
    const at = desktop(p), box = await canvasBox();
    await page.mouse.move(box.x + at[0], box.y + at[1]);
    assert(await sinkFocused(), 'the sink has the focus');
    await records();
    await page.mouse.down({ button: 'left' });
    await page.waitForTimeout(50);
    await page.mouse.down({ button: 'right' });
    await page.waitForTimeout(50);
    await page.mouse.up({ button: 'right' });
    await page.waitForTimeout(50);
    await page.mouse.up({ button: 'left' });
    await page.waitForTimeout(200);
    const got = kinds(await records(), PRESS, RELEASE, FOCUS).map(r => r.slice(0, 2).concat(r[3]));
    assert(JSON.stringify(got) === '[[2,1,1],[2,3,5],[3,3,1],[3,1,0]]',
           'press 1, press 3, release 3, release 1 as [type, button, buttons held], and no focus record: ' + JSON.stringify(got));
    assert(await sinkFocused(), 'the sink keeps the focus');
    p = await probe('the world after the chord', p => p.seq);
    if (p.menus.length) {
      await page.keyboard.press('Escape');
      p = await probe('no menu', p => p.menus.length === 0);
    }
  });

  await check('losing the focus releases the keys and buttons held, then says so', async () => {
    const at = desktop(p), box = await canvasBox();
    await page.mouse.move(box.x + at[0], box.y + at[1]);
    await page.keyboard.down('Shift');
    await page.mouse.down();
    await page.waitForTimeout(100);
    await records();
    await page.evaluate(() => document.getElementById('sink').blur());
    const got = kinds(await records(), KEY_UP, RELEASE, FOCUS);
    await page.mouse.up();
    await page.keyboard.up('Shift');
    assert(JSON.stringify(got) === JSON.stringify([[KEY_UP, 0x400000e1, 0, 1, 225, 0], [RELEASE, 1, 0, 0, 0, 0], [FOCUS, 0, 0, 0, 0, 0]]),
           'key up Shift without modifiers, release 1, focus 0, as [type, a, mods, buttons, c, d]: ' + JSON.stringify(got));
    // a click gives it back
    await click(at);
    await page.waitForFunction(() => document.activeElement === document.getElementById('sink'));
    assert(kinds(await records(), FOCUS).some(r => r[1] === 1), 'focus 1 again');
    // Shift and a press on the desktop may open the menu of the morphs there
    p = await probe('the world after the blur', p => p.seq);
    if (p.menus.length) {
      await page.keyboard.press('Escape');
      p = await probe('no menu', p => p.menus.length === 0);
    }
  });

  await check('Browse > Playground, typing 3 + 4, Ctrl+P shows 7', async () => {
    await openPlayground();
    await click(center(p.playground.bounds));
    await page.waitForTimeout(300);
    const keys = (await stats()).keyToFrame.length;
    for (const ch of '3 + 4') {
      const n = await frames();
      await page.keyboard.type(ch);
      await page.waitForFunction(n => window.PharoWorld.stats.frames > n, n);
    }
    p = await probe('the typed text', p => p.playground.text === '3 + 4');
    const before = await shoot();
    await page.keyboard.press('Control+p');
    p = await probe('7 printed', p => p.printed.includes('7'));
    assert(p.playground.text === '3 + 4', 'the Playground keeps its text: ' + JSON.stringify(p.playground.text));
    assert(region(await shoot(), p.playground.bounds) !== region(before, p.playground.bounds), 'the canvas shows it');
    const k = (await stats()).keyToFrame.slice(keys);
    console.log(`  # keystroke to canvas: median ${median(k).toFixed(1)} ms, max ${Math.max(...k).toFixed(1)} ms, ${k.length} keys`);
  });

  await check('typing 56 more keys, each painted, and Ctrl+P prints what they say', async () => {
    await click(center(p.playground.bounds));          // closes the popover
    p = await probe('no popover', p => p.printed.length === 0);
    await page.keyboard.press('Control+a');
    const text = 'Smalltalk version size + 1000 factorial printString size';
    const keys = (await stats()).keyToFrame.length;
    for (const ch of text) {
      const n = await frames();
      await page.keyboard.type(ch);
      await page.waitForFunction(n => window.PharoWorld.stats.frames > n, n);
    }
    p = await probe('the typed text', p => p.playground.text === text);
    await page.keyboard.press('Control+p');
    p = await probe('a number printed', p => p.printed.some(s => /^\d+$/.test(s)));
    const k = (await stats()).keyToFrame.slice(keys);
    console.log(`  # keystroke to canvas: median ${median(k).toFixed(1)} ms, max ${Math.max(...k).toFixed(1)} ms, ${k.length} keys`);
    assert(k.length >= 50, k.length + ' keys timed');
  });

  await check('a paste: Ctrl+V waits for the text of the paste event', async () => {
    await click(center(p.playground.bounds));          // closes the popover
    p = await probe('no popover', p => p.printed.length === 0);
    await page.keyboard.press('Control+a');
    await paste('6 * 7');
    p = await probe('the pasted text', p => p.playground.text === '6 * 7');
  });

  // The windows that draw with cairo, which the probe opens on request
  const cairo = (manifest.libraries || []).includes('libcairo.so.2');
  let requests = 0;
  // the scene name, once it ran and its windows opened (as opened says)
  async function scene(name, opened, timeout = 60000) {
    const n = ++requests;
    // (the worker answers with an fs-result, which world.js ignores: no
    // request of its own has that id)
    await page.evaluate(name => window.__worker.postMessage({ type: 'fs', op: 'writeFile', path: '/pharo/probe-scene.txt',
                                                               data: name, id: -1 }), name);
    p = await probe('the scene ' + name, p => p.scene && p.scene.request === n && p.scene.done && opened(p.scene.windows, p.scene), timeout);
    return p;
  }
  // the windows of the scene that draw with Athens, and have drawn
  const drawn = ws => ws.filter(w => (w.athens || []).some(a => a.surface));
  if (!cairo) console.log('  # skip the windows that draw with cairo: manifest.json lists no libcairo.so.2 in its libraries');
  if (cairo) await check('an Inspector on a Roassal canvas shows its Canvas, drawn with cairo', async () => {
    const t1 = Date.now();
    p = await scene('roassal', ws => drawn(ws).length > 0);
    console.log('  # the Inspector drew its Canvas in ' + (Date.now() - t1) + ' ms');
    const wrong = sceneWrong(p);
    assert(!wrong, wrong);
    // the frames of the canvas came by now (the probe runs every 100 ms)
    await page.waitForTimeout(500);
    const w = drawn(p.scene.windows)[0], green = pixelsOf(await shoot(), w.bounds, [0, 255, 0]);
    assert(green >= 400, `the canvas shows the green box of the Roassal canvas in ${w.label} at ${w.bounds}: ${green} green pixels`);
    console.log(`  # ${green} green pixels of the box in ${w.label}`);
    if (t.shots) await page.screenshot({ path: t.shot('world-roassal.png') });
  });
  if (cairo) await check('Code Changes (Epicea) opens, and its graph draws', async () => {
    p = await scene('epicea', ws => drawn(ws).length > 0);
    const wrong = sceneWrong(p);
    assert(!wrong, wrong);
  });
  if (cairo) await check('the color picker of the Settings opens, drawn with Roassal (when the image has it: Pharo 12)', async () => {
    p = await scene('colorPicker', (ws, s) => s.absent || drawn(ws).length > 0);
    if (p.scene.absent) {
      assert(p.major > 12, 'Pharo ' + p.major + ' has no SpColorPickerWindow');
      console.log('  # Pharo ' + p.major + ' has no SpColorPickerWindow');
    }
    const wrong = sceneWrong(p);
    assert(!wrong, wrong);
  });
  if (cairo) await check('the windows of the scenes close', async () => {
    p = await scene('close', ws => ws.length === 0);
    assert(!p.scene.error, 'it raised ' + p.scene.error);
  });

  await check('a viewport resize resizes the world', async () => {
    await page.setViewportSize({ width: 1000, height: 700 });
    const box = await canvasBox();
    p = await probe('a World of the new size', p => p.world[0] === Math.round(box.width) && p.world[1] === Math.round(box.height));
    await page.waitForFunction(([w, h]) => {
      const c = document.getElementById('world');
      return c.width === w && c.height === h;
    }, p.world);
    await page.setViewportSize({ width: 1280, height: 800 });
    const back = await canvasBox();
    p = await probe('a World of the first size', p => p.world[0] === Math.round(back.width) && p.world[1] === Math.round(back.height));
  });

  await check('toolbar Stop during a busy loop opens a debugger', async () => {
    await doIt('[ true ] whileTrue');
    await waitState('busy', 15000);
    const t1 = Date.now();
    await page.click('#stop');
    p = await probe('a debugger', p => p.debugger, 30000);
    console.log('  # Stop to debugger (probe) in ' + (Date.now() - t1) + ' ms');
    await waitState('running');
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
    const sizes = {}, saved = {};
    for (const d of files) {
      const file = saved[d.suggestedFilename()] = path.join(tmp, t.name + '-' + d.suggestedFilename());
      await d.saveAs(file);
      sizes[d.suggestedFilename()] = fs.statSync(file).size;
    }
    assert(sizes['Pharo.image'] >= 50e6 && sizes['Pharo.changes'] > 0, 'files ' + JSON.stringify(sizes));
    downloaded = { image: saved['Pharo.image'], changes: saved['Pharo.changes'] };
  });

  await check('a long primitive that never yields shows Busy; Stop then replaces the worker', async () => {
    // findString: in a long string is one primitive, without a single check
    // for interrupts: no slice ends, nothing comes, and the 3 s watchdog
    // replaces the worker.  A forked process starts it when its Delay ends,
    // so the slice before slept, and only the silence of the worker says
    // that the VM is busy
    await openPlayground();
    await doIt("[ (Delay forMilliseconds: 2500) wait. (String new: 1000000 withAll: $a) findString: (String new: 8000 withAll: $a), 'b' startingAt: 1 ] fork");
    await page.waitForTimeout(1500);
    assert(await page.evaluate(() => window.PharoWorld.state) === 'running', 'idle before the primitive');
    await waitState('busy', 15000);
    const t1 = Date.now(), starts = (await stats()).starts;
    await page.click('#stop');
    await page.waitForFunction(n => /VM restarted/.test(document.getElementById('notice-text').textContent) &&
                               !document.getElementById('notice').hidden &&
                               window.PharoWorld.stats.starts > n && window.PharoWorld.stats.frames > 0,
                               starts, { timeout: 90000 });
    const ms = Date.now() - t1;
    console.log('  # replaced, and painted again, in ' + ms + ' ms');
    assert(ms >= 2900, 'not before the 3 s watchdog');
    await waitState('running');
    p = await probe('the world again', p => p.driver === 'OSWebDriver' && p.menubar.length);
  });

  await check('Smalltalk exit: 3 shows the exit and offers Restart, which boots the world again', async () => {
    await openPlayground();
    await doIt('Smalltalk exit: 3');
    await waitState('exited');
    assert(await page.textContent('#status-text') === 'Exited (3)', 'status ' + await page.textContent('#status-text'));
    assert(/exit code 3/.test(await page.textContent('#notice-text')) && await page.textContent('#notice-action') === 'Restart',
           'the notice ' + await page.textContent('#notice-text'));
    assert(await page.isDisabled('#stop') && await page.isDisabled('#save') && !(await page.isDisabled('#download')),
           'Stop and Save off, Download on');
    const starts = (await stats()).starts;
    await page.click('#notice-action');
    await page.waitForFunction(n => window.PharoWorld.stats.starts > n && window.PharoWorld.stats.frames > 0, starts,
                               { timeout: 60000 });
    await waitState('running');
    p = await probe('the world again', p => p.driver === 'OSWebDriver');
  });

  await check('with input not saved the page asks before it goes; another tab\'s save offers Save', async () => {
    await openPlayground();
    assert(await page.evaluate(() => window.PharoWorld.unsaved) && await asksBeforeUnload(), 'unsaved, and beforeunload asks');
    // as vm-worker.js says when another tab saved over the image it booted
    await page.evaluate(() => window.__worker.dispatchEvent(new MessageEvent('message', { data: { type: 'superseded' } })));
    assert(/Another tab has replaced the image/.test(await page.textContent('#notice-text')) &&
           await page.textContent('#notice-action') === 'Save', 'the notice ' + await page.textContent('#notice-text'));
    await records();
    await page.click('#notice-action');
    assert(kinds(await records(), 11).length === 1, 'a save request');
    await page.waitForFunction(() => /saved in this browser/.test(document.getElementById('notice-text').textContent) &&
                               !document.getElementById('notice').hidden, null, { timeout: 120000 });
    await waitState('running');
    assert(!(await page.evaluate(() => window.PharoWorld.unsaved)) && !(await asksBeforeUnload()), 'saved: beforeunload lets the page go');
  });

  await check('Save, reload: the world boots from IndexedDB, as it was saved', async () => {
    await openPlayground();
    await click(center(p.playground.bounds));
    await paste('#savedInThisBrowser');
    p = await probe('the marker in the Playground', p => p.playground.text === '#savedInThisBrowser');
    await page.click('#save');
    await page.waitForFunction(() => /saved in this browser/.test(document.getElementById('notice-text').textContent) &&
                               !document.getElementById('notice').hidden, null, { timeout: 120000 });
    await waitState('running');
    const n = t.requests.length, dialogs = [];
    const dialog = d => { dialogs.push(d.type()); d.accept(); };
    page.on('dialog', dialog);
    try {
      await page.reload();
      await page.waitForFunction(() => window.PharoWorld && window.PharoWorld.stats.frames > 0, null, { timeout: 60000 });
    } finally { page.off('dialog', dialog); }
    assert(!dialogs.length, 'saved, so no dialog: ' + dialogs);
    const served = t.requests.slice(n);
    assert(!served.includes(imageUrl), 'the image is not fetched again: ' + served.join(' '));
    assert(/Started the image saved in this browser/.test(await page.textContent('#notice-text')), 'the note');
    p = await probe('the Playground, as saved', p => p.playground && p.playground.text === '#savedInThisBrowser');
  });

  await check('a reload after input not saved asks first (beforeunload)', async () => {
    await waitState('running');
    await click(center(p.playground.bounds));
    await page.keyboard.type('x');
    await page.waitForFunction(() => window.PharoWorld.unsaved);
    const dialogs = [];
    const dialog = d => { dialogs.push(d.type()); d.accept(); };
    page.on('dialog', dialog);
    try {
      await page.reload();
      await page.waitForFunction(() => window.PharoWorld && window.PharoWorld.stats.frames > 0, null, { timeout: 60000 });
    } finally { page.off('dialog', dialog); }
    assert(dialogs.join() === 'beforeunload', 'one beforeunload dialog: ' + dialogs);
    await waitState('running');
    p = await probe('the world, as saved', p => p.playground && p.playground.text === '#savedInThisBrowser');
  });

  await check('what cannot be opened: the notice says why, offers another, and the world goes on', async () => {
    const notImage = path.join(tmp, 'notes.image');
    fs.writeFileSync(notImage, 'these are notes, not an image');
    const starts = (await stats()).starts;
    const dialogs = await accepting(async () => {
      await page.setInputFiles('#open-file', notImage);
      await noticeSays(/^notes\.image is not a Pharo image that this VM can run \(its header says format \d+, not 68021\)\.$/);
    });
    assert(dialogs.length === 1 && /^Replace the image saved in this browser with notes\.image\?$/.test(dialogs[0]), 'asked ' + dialogs);
    assert(await page.textContent('#notice-action') === 'Open another' && await page.isVisible('#notice-action'), 'Open another');
    assert((await stats()).starts === starts && await page.evaluate(() => window.PharoWorld.state) === 'running', 'the world goes on');
    p = await probe('the world still', p => p.driver === 'OSWebDriver');
  });

  await check('Open a stock Pharo zip: unpacked, prepared for the world, saved with its .sources, and the world opens', async () => {
    const z = zipStock(t.webDir);
    const starts = (await stats()).starts, n = t.requests.length, t1 = Date.now();
    await clearLog();
    const dialogs = await accepting(async () => {
      await page.setInputFiles('#open-file', z.path);
      await page.waitForFunction(n => window.PharoWorld.stats.starts >= n + 2 && window.PharoWorld.stats.frames > 0, starts,
                                 { timeout: 240000 });
    });
    console.log('  # unpacked, prepared, and painted in ' + (Date.now() - t1) + ' ms');
    assert(dialogs.length === 1 && dialogs[0] === 'Replace the image saved in this browser with ' + z.name + '?', 'asked ' + dialogs);
    await waitState('running');
    const l = await log();
    assert(l.notices.some(s => /^Unpacking stock12\.zip… [1-9]\d?%$/.test(s)), 'the progress ' + JSON.stringify(l.notices.slice(0, 4)));
    assert(l.states.includes('preparing') && l.overlays.some(s => /^Preparing the image for the world/.test(s)),
           'Preparing shown: ' + JSON.stringify(l.states) + ' ' + JSON.stringify(l.overlays));
    const prepared = `The image is prepared for the world (OSWindow-Web and ${fontsNamed(manifest.fonts)} fonts), and saved in this browser.`;
    assert((await page.textContent('#notice-text')).startsWith(prepared), 'the notice ' + await page.textContent('#notice-text'));
    const served = t.requests.slice(n);
    assert(!served.includes(imageUrl) && !served.includes(sourcesUrl), 'fetched ' + served.join(' '));
    p = await probe('the world of the prepared image', p => p.driver === 'OSWebDriver' && p.menubar.length);
    const wrong = wrongFonts(p, manifest.fonts);
    assert(!wrong, 'the page prepared ' + wrong);
    const size = await page.evaluate(f => window.PharoWorld.readFile(f).then(d => d.length), '/pharo/' + siteSources.path);
    assert(size === z.ownSize, 'its own .sources: ' + size);
  });

  await check('reload: the prepared image boots from IndexedDB, with its own .sources', async () => {
    const z = zipStock(t.webDir), n = t.requests.length;
    await clearLog();
    const dialogs = await accepting(async () => {
      await page.reload();
      await page.waitForFunction(() => window.PharoWorld && window.PharoWorld.stats.frames > 0, null, { timeout: 60000 });
    });
    assert(!dialogs.length, 'dialogs ' + dialogs);
    const served = t.requests.slice(n);
    assert(!served.includes(imageUrl) && !served.includes(sourcesUrl), 'fetched ' + served.join(' '));
    assert(!(await log()).states.includes('preparing'), 'prepared already');
    assert(/Started the image saved in this browser/.test(await page.textContent('#notice-text')), 'the note');
    p = await probe('the world', p => p.driver === 'OSWebDriver' && p.menubar.length);
    const size = await page.evaluate(f => window.PharoWorld.readFile(f).then(d => d.length), '/pharo/' + siteSources.path);
    assert(size === z.ownSize, 'its own .sources: ' + size);
  });

  await check('a drop of the downloaded world image and its .changes boots it at once, and keeps it', async () => {
    assert(downloaded, 'nothing downloaded');
    const starts = (await stats()).starts, n = t.requests.length;
    await clearLog();
    let drop;
    const dialogs = await accepting(async () => {
      drop = await dropFiles(page, [downloaded.image, downloaded.changes]);
      await page.waitForFunction(n => window.PharoWorld.stats.starts > n && window.PharoWorld.stats.frames > 0, starts, { timeout: 90000 });
      await noticeSays(/^The opened image is now kept in this browser\.$/, 60000);
    });
    assert(drop.shown && drop.took && drop.hidden, 'the drop ' + JSON.stringify(drop));
    assert(dialogs.length === 1 && /^Replace the image saved in this browser with [^ ]*Pharo\.image\?$/.test(dialogs[0]), 'asked ' + dialogs);
    assert((await stats()).starts === starts + 1 && !(await log()).states.includes('preparing'), 'no preparation');
    assert((await log()).notices.some(s => /^Started the opened image, [^ ]*Pharo\.image\.$/.test(s)), 'the note');
    assert(t.requests.slice(n).includes(sourcesUrl) && !t.requests.slice(n).includes(imageUrl), 'the .sources of the site');
    await waitState('running');
    p = await probe('the world of the dropped image', p => p.driver === 'OSWebDriver' && p.menubar.length);
  });

  if (t.shots) {
    await page.keyboard.press('Escape');
    for (const scheme of ['light', 'dark']) {
      await page.emulateMedia({ colorScheme: scheme });
      await page.setViewportSize({ width: 1280, height: 800 });
      await page.waitForTimeout(800);             // the world follows the size
      await page.screenshot({ path: t.shot(`world-${scheme}-1280.png`) });
      await page.setViewportSize({ width: 360, height: 740 });
      await page.waitForTimeout(800);
      await page.screenshot({ path: t.shot(`world-${scheme}-360.png`) });
    }
    await page.emulateMedia({ colorScheme: 'light' });
    await page.setViewportSize({ width: 1280, height: 800 });
  }

  await check('dark and light: prefers-color-scheme sets the page around the world', async () => {
    const bg = () => page.evaluate(() => getComputedStyle(document.body).backgroundColor);
    await page.emulateMedia({ colorScheme: 'light' });
    const light = await bg();
    await page.emulateMedia({ colorScheme: 'dark' });
    const dark = await bg();
    assert(light !== dark && light !== 'rgba(0, 0, 0, 0)' && dark !== 'rgba(0, 0, 0, 0)', `backgrounds ${light} ${dark}`);
    await page.emulateMedia({ colorScheme: 'light' });
  });

  for (const width of [360, 320]) await check(`no horizontal scroll at ${width} px, 16 px gutters, the world follows`, async () => {
    await page.setViewportSize({ width, height: 740 });
    const box = await canvasBox();
    p = await probe('a World of the narrow canvas', p => p.world[0] === Math.round(box.width) && p.world[1] === Math.round(box.height));
    const o = await page.evaluate(() => {
      const d = document.documentElement;
      const wide = [...document.querySelectorAll('body *')].filter(e => {
        const r = e.getBoundingClientRect();
        return r.width && (r.right > d.clientWidth + 0.5 || r.left < -0.5) && getComputedStyle(e).visibility !== 'hidden';
      }).map(e => e.tagName + (e.id ? '#' + e.id : ''));
      return { sw: d.scrollWidth, cw: d.clientWidth, bw: document.body.scrollWidth, wide: wide.slice(0, 5) };
    });
    assert(o.sw <= o.cw && o.bw <= o.cw && !o.wide.length, JSON.stringify(o));
    const gutter = await page.$eval('#stage', e => [e.getBoundingClientRect().left,
                                                    document.documentElement.clientWidth - e.getBoundingClientRect().right]);
    assert(gutter.every(g => g >= 15.5 && g <= 16.5), '16 px gutters, got ' + gutter);
    await page.setViewportSize({ width: 1280, height: 800 });
  });

  await check('without OffscreenCanvas the page says so, and points to the Console', async () => {
    const f = await t.context.newPage();
    t.watch(f);
    await f.addInitScript(() => { delete HTMLCanvasElement.prototype.transferControlToOffscreen; });
    await f.goto(t.base + 'world.html');
    await f.waitForFunction(() => document.getElementById('status').dataset.state === 'error', null, { timeout: 10000 });
    assert(/OffscreenCanvas/.test(await f.textContent('#notice-text')), 'notice: ' + await f.textContent('#notice-text'));
    assert(await f.isVisible('#notice-link') && await f.getAttribute('#notice-link', 'href') === 'index.html', 'a link to the Console');
    await f.close();
  });

  await check('from file:// the page explains that it needs HTTP', async () => {
    const f = await t.context.newPage();
    t.watch(f);
    await f.goto('file://' + path.join(t.webDir, 'world.html'));
    await f.waitForFunction(() => document.getElementById('status').dataset.state === 'error', null, { timeout: 10000 });
    assert(/served over HTTP/.test(await f.textContent('#notice-text')), 'notice: ' + await f.textContent('#notice-text'));
    await f.close();
  });

  await check('no worker warned of an engine error or an exception of the display', async () => {
    assert(!diag.length, diag.slice(0, 5).join(' | '));
  });
});
