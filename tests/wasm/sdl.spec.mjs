// sdl.spec.mjs - the world through SDL2 in a browser (build-wasm/web/sdl.html)
//
// usage: node sdl.spec.mjs WEB_DIR   (see tests/wasm/lib/pw.mjs)
//
// For a build with SDL2 (manifest.json says "sdl2": true: WASM_SDL2=ON;
// make wasm-check-browser skips this spec otherwise).  The page sdl.html
// runs the VM's worker in its 'sdl' mode: without a display, the image
// opens its world through its own OSSDL2Driver, and SDL2's Emscripten
// video driver draws it into the canvas that the page transferred to the
// worker, through the DOM shim of sdl-shim.js; the page forwards its DOM
// events, which SDL's handlers queue for the image.  As in world.spec.mjs,
// the worlds boot with tests/wasm/st/world-probe.st as well (the spec
// serves a vm-driver.js that adds it to the arguments and the files of the
// VM), whose /pharo/probe.json the spec reads through the page
// (PharoSDL.readFile, PharoWorld.readFile), and it drives the page with
// real pointer and keyboard events:
//
//   - the world of the site's image paints, through OSSDL2Driver, at the
//     size of the canvas, which shows many colours;
//   - a right click on the desktop opens the world menu, which the canvas
//     shows, and Escape closes it;
//   - Browse > Playground opens a Playground, '3 + 4' typed into it comes as
//     text (SDL_TEXTINPUT, from the keypresses that the page lets the
//     browser send), and Ctrl+A and Ctrl+P print 7;
//   - idle, what the VM costs a second (slices, and the time spent in
//     them, which the vm-driver.js of the spec measures in the worker), and
//     the presents of SDL (PharoSDL.costs()): printed, with the ratio to
//     the world page below, never a failure.  The page is loaded again for
//     it, with a vm-driver.js that does not add the probe, which costs
//     time of its own every 100 ms;
//   - a stock image opened on the page (a zip of WASM_DIR/image/stock, as
//     files.pharo.org has them) opens its world through OSSDL2Driver too,
//     unprepared;
//   - the git proxy that the Console's Settings keep (pharo-wasm.gitProxy,
//     set from another page of the site) goes to the running VM when it
//     changes, '' when it is not one that the Console takes, and to the
//     worker of a Restart in its init;
//   - the negative control: world.html on the same build still runs the
//     world on OSWebDriver; and its idle cost is measured the same way,
//     on a load without the probe.
//
// With the last check of pw.mjs (no console errors, no failed requests,
// and no exception in a page or a worker), 9 checks for each browser.
// Exits with status 1 if any check fails, the page logs an error, or a
// worker warns of an engine error.

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { run, decodePNG } from './lib/pw.mjs';
import { imageZip } from './lib/zip.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const webDir = path.resolve(process.argv[2] || 'build-wasm/web');
const manifest0 = JSON.parse(fs.readFileSync(path.join(webDir, 'manifest.json'), 'utf8'));
if (manifest0.sdl2 !== true) {
  console.log(`skip sdl.spec.mjs: ${webDir}/manifest.json has no SDL2 (WASM_SDL2=OFF)`);
  process.exit(0);
}
const scratch = process.env.TEST_DIR || os.tmpdir();
fs.mkdirSync(scratch, { recursive: true });
const tmp = fs.mkdtempSync(path.join(scratch, 'sdl-spec-'));
process.on('exit', () => fs.rmSync(tmp, { recursive: true, force: true }));
const probeSource = fs.readFileSync(path.join(here, 'st', 'world-probe.st'), 'utf8');
// Appended to vm-driver.js: the worlds ('sdl' and 'world') boot with the
// probe, when probing; and in the worker, the slices of the VM are counted
// and timed, as the shim of sdl.html does it, which a message
// 'sdlspec-costs' asks for
const driverPatch = probing => `
;(function () {
  // sdl.spec.mjs: boot the worlds with tests/wasm/st/world-probe.st (${probing}), and time the slices
  var D = self.PharoVMDriver, start = D.start, vmArgs = D.vmArgs, probe = ${JSON.stringify(probeSource)};
  var probing = ${probing}, costs = { slices: 0, sliceMs: 0 };
  D.vmArgs = function (mode, image) {
    var args = vmArgs(mode, image);
    return probing && (mode === 'world' || mode === 'sdl') ? args.concat(['st', '/pharo/st/world-probe.st']) : args;
  };
  D.start = function (create, o) {
    if (probing) o.files = (o.files || []).concat([{ path: '/pharo/st/world-probe.st', data: probe }]);
    return start(create, o).then(function (vm) {
      if (vm) {
        var M = vm.module, resume = M._vm_resume;
        M._vm_resume = function () {
          var t = performance.now();
          try { return resume.apply(this, arguments); } finally { costs.slices++; costs.sliceMs += performance.now() - t; }
        };
      }
      return vm;
    });
  };
  if (typeof WorkerGlobalScope !== 'undefined')
    self.addEventListener('message', function (e) {
      if (e.data && e.data.type === 'sdlspec-costs')
        postMessage({ type: 'sdlspec-costs', slices: costs.slices, sliceMs: costs.sliceMs, at: performance.now() });
    });
})();
`;

// What the warnings of the workers must not say
const ENGINE_ERRORS = /RuntimeError|RangeError|Aborted|unreachable|signature_mismatch|unsupported syscall|ReferenceError|TypeError/;
const IDLE_MS = 5000;                   // the idle time measured
const SETTLE_MS = 3000;                 // the world settles so long before

const center = b => [(b[0] + b[2]) / 2, (b[1] + b[3]) / 2];
const inside = (p, b) => p[0] >= b[0] && p[0] < b[2] && p[1] >= b[1] && p[1] < b[3];

// The stock image of the build, zipped as files.pharo.org does; written once
let stockZip = null;
function zipStock(webDir) {
  if (stockZip) return stockZip;
  const stock = path.join(webDir, '..', 'image', 'stock');
  stockZip = { path: path.join(tmp, 'stock.zip'), name: 'Pharo-stock-sdl.image' };
  fs.writeFileSync(stockZip.path, imageZip(stock, { base: 'Pharo-stock-sdl', folder: 'stock' }));
  return stockZip;
}

await run(async t => {
  const { page, check, assert, manifest } = t;

  // (whether the worlds that boot from now on get the probe)
  let probing = true;
  await t.context.route('**/vm-driver.js*', async route => {
    const body = fs.readFileSync(path.join(t.webDir, 'vm-driver.js'), 'utf8');
    route.fulfill({ contentType: 'text/javascript', body: body + driverPatch(probing) });
  });
  // the pages log a crash of the VM, with the Smalltalk stacks, as a warning
  page.on('console', m => {
    if (/^pharo: /.test(m.text())) console.log('  # the VM crashed: ' + m.text().split('\n').slice(0, 12).join('\n  #   '));
  });
  // vm-worker.js passes on what the runtime says (onDiag) as warnings
  const diag = [];
  page.on('worker', w => w.on('console', m => { if (ENGINE_ERRORS.test(m.text())) diag.push(m.text()); }));
  // the worker of the page, for the costs that the patch measures; and the
  // git proxy that the page gives it
  await page.addInitScript(() => {
    const post = Worker.prototype.postMessage;
    window.__gitPosts = [];
    Worker.prototype.postMessage = function (m, transfer) {
      window.__worker = this;
      if (m && (m.type === 'init' || m.type === 'gitProxy')) window.__gitPosts.push({ type: m.type, gitProxy: m.gitProxy });
      return post.call(this, m, transfer);
    };
    window.__costs = () => new Promise((resolve, reject) => {
      const w = window.__worker;
      if (!w) { reject(new Error('no worker')); return; }
      const got = e => {
        if (!e.data || e.data.type !== 'sdlspec-costs') return;
        w.removeEventListener('message', got);
        resolve(e.data);
      };
      w.addEventListener('message', got);
      w.postMessage({ type: 'sdlspec-costs' });
    });
  });

  // which page: 'sdl' (PharoSDL, #screen) or 'world' (PharoWorld, #world)
  let on = 'sdl';
  const api = () => on === 'sdl' ? 'PharoSDL' : 'PharoWorld';
  const canvasLocator = () => page.locator(on === 'sdl' ? '#screen' : '#world');
  const canvasBox = () => canvasLocator().boundingBox();
  const shoot = async () => decodePNG(await canvasLocator().screenshot());
  const sdlStats = () => page.evaluate(() => window.PharoSDL.stats);
  const waitState = (st, timeout = 30000) =>
    page.waitForFunction(([name, s]) => window[name] && window[name].state === s, [api(), st], { timeout });
  // the canvas's pixels in a rectangle of the world, as a string of RGBA
  const region = (shot, b) => {
    const parts = [];
    for (let y = Math.max(0, b[1]); y < Math.min(shot.height, b[3]); y++)
      parts.push(shot.data.subarray((y * shot.width + Math.max(0, b[0])) * 4, (y * shot.width + Math.min(shot.width, b[2])) * 4).toString('hex'));
    return parts.join('');
  };
  // how many colours a screenshot has, and the share of the commonest
  const colours = shot => {
    const counts = new Map();
    for (let i = 0; i < shot.data.length; i += 4) {
      const k = shot.data.readUInt32LE(i);
      counts.set(k, (counts.get(k) || 0) + 1);
    }
    return { count: counts.size, top: Math.max(...counts.values()) / (shot.width * shot.height) };
  };

  // The probe written after the call that pred accepts
  async function probe(what, pred, timeout = 30000) {
    const t0 = Date.now();
    let first = null, last = null;
    for (;;) {
      let p = null;
      try {
        p = JSON.parse(await page.evaluate(name => window[name].readFile('/pharo/probe.json')
          .then(d => new TextDecoder().decode(d)), api()));
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
  // A point of the desktop, where no window is, from the right of the world
  const desktop = p => {
    for (let x = p.world[0] - 30; x > 30; x -= 20) {
      const at = [x, Math.round(p.world[1] / 2)];
      if (!p.windows.some(w => inside(at, w.bounds))) return at;
    }
    throw new Error('no free point of the desktop: ' + JSON.stringify(p.windows.map(w => w.bounds)));
  };
  // What the VM costs a second, idle, without the probe: {slices, ms} a
  // second, on the page loaded again, once painted is true
  async function idle(painted) {
    probing = false;
    try {
      await page.reload();
      await page.waitForFunction(painted, null, { timeout: 90000 });
      await waitState('running');
    } finally {
      probing = true;
    }
    await page.waitForTimeout(SETTLE_MS);
    // (and on sdl.html, what the shim measured meanwhile: the presents)
    const shim = () => on === 'sdl' ? page.evaluate(() => window.PharoSDL.costs()) : null;
    const a = await page.evaluate(() => window.__costs()), sa = await shim();
    await page.waitForTimeout(IDLE_MS);
    const b = await page.evaluate(() => window.__costs()), sb = await shim();
    const s = (b.at - a.at) / 1000;
    const costs = sa && { presents: sb.presents - sa.presents, presentMs: sb.presentMs - sa.presentMs, pixels: sb.pixels - sa.pixels };
    return { slices: (b.slices - a.slices) / s, ms: (b.sliceMs - a.sliceMs) / s, costs };
  }
  // the dialogs of f, answered with accept
  async function accepting(f) {
    const dialogs = [];
    const dialog = d => { dialogs.push(d.message() || d.type()); d.accept(); };
    page.on('dialog', dialog);
    try { await f(); } finally { page.off('dialog', dialog); }
    return dialogs;
  }

  // Browse > Playground: a new Playground, in front
  async function openPlayground() {
    p = await probe('the world', p => p.menubar.length);
    const n = p.windows.filter(w => w.label === 'Playground').length;
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

  let p, t0, sdlIdle = null;
  await check('the world paints through SDL2 within 90 s, with many colours', async () => {
    assert(manifest.sdl2 === true, 'manifest.json says "sdl2": true, not ' + JSON.stringify(manifest.sdl2));
    t0 = Date.now();
    await page.goto(t.base + 'sdl.html');
    await page.waitForFunction(() => window.PharoSDL && window.PharoSDL.stats.paintedAt > 0, null, { timeout: 90000 });
    const s = await sdlStats(), costs = await page.evaluate(() => window.PharoSDL.costs());
    console.log(`  # first present ${(s.paintedAt - s.startedAt).toFixed(0)} ms after the worker started ` +
                `(${Date.now() - t0} ms here, nothing cached); SDL's first present ${costs.firstPresentMs.toFixed(0)} ms after the shim`);
    await waitState('running');
    assert(await page.isHidden('#overlay'), 'the loading card is gone');
    p = await probe('the world', p => p.menubar.length);
    const shot = await shoot(), c = colours(shot);
    assert(c.count > 64 && c.top < 0.9, `a non-uniform canvas: ${c.count} colours, the commonest ${(100 * c.top).toFixed(1)}%`);
  });

  await check('the world runs on OSSDL2Driver, at the size of the canvas', async () => {
    p = await probe('the world', p => p.world);
    assert(p.driver === 'OSSDL2Driver' && p.renderer === 'OSWorldRenderer', `renderer ${p.renderer}, driver ${p.driver}`);
    const box = await canvasBox(), s = await sdlStats();
    assert(p.world[0] === Math.round(box.width) && p.world[1] === Math.round(box.height),
           `the World ${p.world} has the size of the canvas ${box.width}x${box.height}`);
    assert(s.sizes.length > 0 && String(s.sizes[s.sizes.length - 1]) === String(p.world), 'the size that SDL gave the canvas: ' + JSON.stringify(s.sizes));
  });

  await check('a right click on the desktop opens the world menu; Escape closes it', async () => {
    const at = desktop(p), before = await shoot();
    await click(at, 'right');
    try {
      p = await probe('the world menu', p => p.menus.length > 0);
      const items = p.menus[0].items.map(i => i.bounds);
      const bounds = items.reduce((b, i) => [Math.min(b[0], i[0]), Math.min(b[1], i[1]), Math.max(b[2], i[2]), Math.max(b[3], i[3])]);
      await page.waitForTimeout(300);
      assert(region(await shoot(), bounds) !== region(before, bounds), 'the canvas shows the menu at ' + bounds);
    } finally {
      await page.keyboard.press('Escape');
    }
    p = await probe('the menu closed', p => p.menus.length === 0);
  });

  await check('Browse > Playground, typing 3 + 4, Ctrl+A and Ctrl+P print 7', async () => {
    await openPlayground();
    await click(center(p.playground.bounds));
    await page.waitForTimeout(300);
    await page.keyboard.type('3 + 4', { delay: 50 });
    p = await probe('the typed text', p => p.playground.text === '3 + 4');
    const before = await shoot();
    await page.keyboard.press('Control+a');
    await page.keyboard.press('Control+p');
    p = await probe('7 printed', p => p.printed.includes('7'));
    assert(p.playground.text === '3 + 4', 'the Playground keeps its text: ' + JSON.stringify(p.playground.text));
    await page.waitForTimeout(300);
    assert(region(await shoot(), p.playground.bounds) !== region(before, p.playground.bounds), 'the canvas shows it');
    assert(!p.debugger, 'no debugger');
    // (every present is the whole window: SDL's framebuffer ignores the dirty rectangles)
    const c = await page.evaluate(() => window.PharoSDL.costs());
    console.log(`  # ${c.presents} presents of SDL so far, of ${Math.round(c.pixels / c.presents)} pixels each, ` +
                `${(c.presentMs / c.presents).toFixed(2)} ms of putImageData each`);
  });

  await check('idle: what the VM and the presents of SDL cost a second', async () => {
    sdlIdle = await idle(() => window.PharoSDL && window.PharoSDL.stats.paintedAt > 0);
    console.log(`  # idle on sdl.html: ${sdlIdle.slices.toFixed(0)} slices/s, ${sdlIdle.ms.toFixed(1)} ms/s in the VM, ` +
                `${sdlIdle.costs.presents} presents of SDL in ${IDLE_MS / 1000} s`);
    assert(sdlIdle.slices > 0, 'the VM runs');
  });

  await check('a stock image opened on the page opens its world through OSSDL2Driver, unprepared', async () => {
    const z = zipStock(t.webDir), starts = (await sdlStats()).starts, t1 = Date.now();
    const dialogs = await accepting(async () => {
      await page.setInputFiles('#open-file', z.path);
      await page.waitForFunction(n => window.PharoSDL.stats.starts > n && window.PharoSDL.stats.paintedAt > 0, starts,
                                 { timeout: 120000 });
    });
    console.log('  # unpacked, booted and painted in ' + (Date.now() - t1) + ' ms');
    assert(dialogs.length === 1 && /^Open .*\? What the world did since it started is lost\.$/.test(dialogs[0]), 'asked ' + dialogs);
    await waitState('running');
    p = await probe('the world of the stock image', p => p.menubar.length);
    assert(p.driver === 'OSSDL2Driver', 'driver ' + p.driver);
    const c = colours(await shoot());
    assert(c.count > 64 && c.top < 0.9, `a non-uniform canvas: ${c.count} colours, the commonest ${(100 * c.top).toFixed(1)}%`);
  });

  await check('the git proxy of the Console goes to the VM when it changes, and to the worker of a Restart', async () => {
    const PROXY = 'http://127.0.0.1:9/';
    const gitPosts = () => page.evaluate(() => window.__gitPosts.splice(0));
    const posted = (type, value) => page.waitForFunction(([type, value]) =>
      window.__gitPosts.some(m => m.type === type && m.gitProxy === value), [type, value], { timeout: 10000 });
    // another page of the site sets it, as the Console's Settings do: a
    // storage event here
    const other = await t.context.newPage();
    const set = async v => {
      await gitPosts();
      await other.evaluate(v => localStorage.setItem('pharo-wasm.gitProxy', v), v);
    };
    try {
      await other.goto(t.base + 'THIRD-PARTY-NOTICES.txt');
      await set(PROXY);
      await posted('gitProxy', PROXY);
      // one with credentials, which the Console does not take: none
      await set('http://u:p@127.0.0.1:9/');
      await posted('gitProxy', '');
      await set(PROXY);
      await posted('gitProxy', PROXY);
      await gitPosts();
      const starts = (await sdlStats()).starts;
      const dialogs = await accepting(async () => {
        await page.click('#restart');
        await page.waitForFunction(n => window.PharoSDL.stats.starts > n && window.PharoSDL.stats.paintedAt > 0, starts,
                                   { timeout: 120000 });
      });
      assert(dialogs.length === 1, 'asked ' + dialogs);
      const inits = (await gitPosts()).filter(m => m.type === 'init');
      assert(inits.length === 1 && inits[0].gitProxy === PROXY, 'the init of the restart: ' + JSON.stringify(inits));
      await waitState('running');
    } finally {
      await other.evaluate(() => localStorage.removeItem('pharo-wasm.gitProxy')).catch(() => {});
      await other.close();
    }
  });

  await check('world.html on the same build runs OSWebDriver', async () => {
    if (!manifest.world) {
      console.log('  # no world image (manifest.json world: false): world.html is not checked');
      return;
    }
    on = 'world';
    await page.goto(t.base + 'world.html');
    await page.waitForFunction(() => window.PharoWorld && window.PharoWorld.stats.frames > 0, null, { timeout: 90000 });
    await waitState('running');
    p = await probe('the world of world.html', p => p.menubar.length);
    assert(p.driver === 'OSWebDriver', 'driver ' + p.driver);
    const w = await idle(() => window.PharoWorld && window.PharoWorld.stats.frames > 0);
    console.log(`  # idle on world.html: ${w.slices.toFixed(0)} slices/s, ${w.ms.toFixed(1)} ms/s in the VM` +
                (sdlIdle ? `; sdl.html costs ${(sdlIdle.ms / w.ms).toFixed(1)} times its VM time, ` +
                           `${(sdlIdle.slices / w.slices).toFixed(1)} times its slices` : ''));
  });

  if (diag.length) t.errors.push(...diag.map(d => 'worker warning: ' + d.split('\n')[0]));
});
