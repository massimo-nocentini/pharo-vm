// world-harness.mjs - the Pharo world of the world page, in node, without a browser
//
// usage: node [v8 flags] world-harness.mjs WEB_DIR
//
// WEB_DIR is a built web directory (build-wasm/web) whose manifest.json says
// world: true, so that its image is the prepared world image.  The harness
// boots it as the world page does: pharo-web.js (the MEMORY64=2 module of the
// pages) through packaging/emscripten/web/vm-driver.js, the files of the
// manifest in MEMFS, and display-worker.js of this tree as Module.webDisplay.
// It has no canvas, so the display paints into its memory framebuffer.  The
// arguments are the world's (vmArgs('world')) plus st tests/wasm/st/world-probe.st
// (Pharo 15 takes a .st file through the st command only; Pharo 12 takes it so too),
// whose probe.json says what the world shows.
//
// The harness first checks the canvas side of display-worker.js, which the
// browser runs, with a stand-in OffscreenCanvas and module.  Then it plays
// the page: it builds the input records with keymap.js from keyboard events
// of a US layout, as world.js does, and hands them and the resizes and
// pastes to PharoDisplay.onMessage, between slices, as vm-worker.js does.
// It runs the world through its fonts (those of manifest.fonts: FreeType,
// with glyphs for a lambda and an arrow, or bitmap fonts), its menus, a
// Playground, print it, the clipboard, a resize, a burst of events, Stop
// during a busy UI process and a save request, prints every case and a
// table of timings (first frame, click, keystroke, resize, Stop and save,
// each until the frame or the probe shows it), and exits with status 1 if
// any case fails.  What the emscripten runtime said (onDiag, where the
// display's own exceptions go) must hold no engine error, and goes to
// stderr.  Lane 80
// (tests/wasm/lanes/80-world-harness.sh) runs it.

import fs from 'node:fs';
import path from 'node:path';
import zlib from 'node:zlib';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const here = path.dirname(fileURLToPath(import.meta.url));
const srcDir = path.join(here, '..', '..');
const webDir = path.resolve(process.argv[2] || 'build-wasm/web');
const web = name => path.join(srcDir, 'packaging', 'emscripten', 'web', name);
const Driver = require(web('vm-driver.js'));
const PharoDisplay = require(web('display-worker.js'));
const Keymap = require(web('keymap.js'));
const createPharoVM = require(path.join(webDir, 'pharo-web.js'));
const { BUSY, HOST_IMAGE_SAVED } = Driver;
const manifest = JSON.parse(fs.readFileSync(path.join(webDir, 'manifest.json'), 'utf8'));
const wasmModule = new WebAssembly.Module(fs.readFileSync(path.join(webDir, 'pharo-web.wasm')));
const probeSource = fs.readFileSync(path.join(here, 'st', 'world-probe.st'));

const WIDTH = 1024, HEIGHT = 768;
// The record types of OSWebDriver
const MOVE = 1, PRESS = 2, RELEASE = 3, KEY_DOWN = 5, KEY_UP = 6, TEXT = 7, SAVE = 11;
// What onDiag must not say: an engine error, or an exception of the display
const ENGINE_ERRORS = /RuntimeError|RangeError|Aborted|unreachable|signature_mismatch|unsupported syscall|webDisplay\./;

if (!manifest.world) {
  console.log(`${webDir}/manifest.json has no world image (world: false)`);
  process.exit(1);
}

// The files of the world page, in MEMFS: what the worker writes there
function worldFiles() {
  const files = manifest.files.map(f => {
    const data = fs.readFileSync(path.join(webDir, f.url));
    return { path: '/pharo/' + f.path, data: data[0] === 0x1f && data[1] === 0x8b ? zlib.gunzipSync(data) : data };
  });
  for (const p of manifest.st || []) files.push({ path: '/pharo/' + p, data: fs.readFileSync(path.join(webDir, p)) });
  files.push({ path: '/pharo/st/world-probe.st', data: probeSource });
  return files;
}

let failures = 0, passes = 0;
const sleep = ms => new Promise(r => setTimeout(r, ms));
const now = () => performance.now();
const timings = {};
const timing = (what, ms) => (timings[what] = timings[what] || []).push(ms);

// The callbacks wake the waiters
let waiters = [];
function notify() { const w = waiters; waiters = []; for (const f of w) f(); }
function nextEvent(ms) {
  return new Promise(resolve => {
    const timer = setTimeout(resolve, ms);
    waiters.push(() => { clearTimeout(timer); resolve(); });
  });
}
async function waitFor(what, pred, ms = 30000) {
  const t0 = now();
  for (;;) {
    if (S && (S.exit || S.crash)) throw new Error((S.exit ? 'exit ' + S.exit.code : 'crash: ' + S.crash.message) +
                                                  ' while waiting for ' + what);
    const v = pred();
    if (v) return v;
    const left = ms - (now() - t0);
    if (left <= 0) throw new Error('timed out after ' + ms + ' ms waiting for ' + what);
    await nextEvent(Math.min(left, 100));
  }
}

let S = null;

// The world: the VM, its display, and what the page would have received
async function session() {
  const s = { out: '', err: '', states: [], host: [], msgs: [], frames: [], exit: null, crash: null, diag: '' };
  const dec = { 1: new TextDecoder(), 2: new TextDecoder() };
  s.display = PharoDisplay.create({ width: WIDTH, height: HEIGHT }, m => {
    s.msgs.push(m);
    if (m.kind === 'frame') s.frames.push({ t: now(), frame: m.frame, width: m.width, height: m.height,
                                            rects: s.display.frame.rects });
    notify();
  });
  s.t0 = now();
  s.drv = await Driver.start(createPharoVM, {
    args: Driver.vmArgs('world', '/pharo/' + manifest.image).concat(['st', '/pharo/st/world-probe.st']),
    files: worldFiles(),
    wasmModule,
    config: { webDisplay: s.display },
    schedule: f => setImmediate(f),
    later: (f, ms) => setTimeout(f, ms),
    onOutput: (fd, bytes) => {
      const text = dec[fd].decode(bytes, { stream: true });
      s.out += text;
      if (fd === 2) s.err += text;
      notify();
    },
    onState: st => { s.states.push({ t: now(), st }); notify(); },
    onHost: (kind, text) => { s.host.push({ t: now(), kind, text }); notify(); },
    onExit: code => { s.exit = { code }; notify(); },
    onCrash: (message, stack, stacks) => { s.crash = { message, stack, stacks }; notify(); },
    onDiag: t => {
      s.diag += t;
      process.stderr.write('[diag] ' + t + (/\n$/.test(t) ? '' : '\n'));
    },
  });
  S = s;
  if (!s.drv) return s;
  // as vm-worker.js does once the VM started, then the page at once: the
  // canvas's size, before the image boots
  s.display.handle({ kind: 'attach' }, s.drv);
  page('resize', { width: WIDTH, height: HEIGHT });
  s.drv.begin();
  return s;
}

// ---- the page

// What vm-worker.js does with a display message of the page
function page(kind, fields) {
  PharoDisplay.onMessage(Object.assign({ type: 'display', kind }, fields), S.drv);
}
const ts = () => Math.round(now()) | 0;
let mouse = { x: 0, y: 0, buttons: 0 };
function record(type, a = 0, c = 0, d = 0, mods = 0) {
  page('event', { event: [type, ts(), mouse.x, mouse.y, a, mods | (mouse.buttons << 8), c, d] });
}
function move(x, y) {
  mouse.x = x;
  mouse.y = y;
  record(MOVE);
}
async function click(x, y, button = 1) {
  move(x, y);
  mouse.buttons |= [0, 1, 2, 4][button];
  record(PRESS, button);
  await sleep(30);
  mouse.buttons &= ~[0, 1, 2, 4][button];
  record(RELEASE, button);
}
const center = b => [Math.round((b[0] + b[2]) / 2), Math.round((b[1] + b[3]) / 2)];

// KeyboardEvents of a US layout, as a browser makes them
const SHIFTED = { '!': 'Digit1', '@': 'Digit2', '#': 'Digit3', $: 'Digit4', '%': 'Digit5', '^': 'Digit6', '&': 'Digit7',
                  '*': 'Digit8', '(': 'Digit9', ')': 'Digit0', _: 'Minus', '+': 'Equal', '{': 'BracketLeft',
                  '}': 'BracketRight', '|': 'Backslash', ':': 'Semicolon', '"': 'Quote', '~': 'Backquote',
                  '<': 'Comma', '>': 'Period', '?': 'Slash' };
const PLAIN = { ' ': 'Space', '-': 'Minus', '=': 'Equal', '[': 'BracketLeft', ']': 'BracketRight', '\\': 'Backslash',
                ';': 'Semicolon', "'": 'Quote', '`': 'Backquote', ',': 'Comma', '.': 'Period', '/': 'Slash' };
function keyEvent(key, mods = {}) {
  let code = key.length === 1 ? PLAIN[key] || SHIFTED[key] : key;
  if (/^[a-z]$/i.test(key)) code = 'Key' + key.toUpperCase();
  if (/^[0-9]$/.test(key)) code = 'Digit' + key;
  return Object.assign({ key, code, shiftKey: key in SHIFTED || /^[A-Z]$/.test(key), ctrlKey: false, altKey: false,
                         metaKey: false, repeat: false, isComposing: false, getModifierState: () => false }, mods);
}
// One key, as world.js sends it: down, its text, up
function press(e) {
  const k = Keymap.key(e, false);
  record(KEY_DOWN, k.sym, k.scan, 0, k.mods);
  if (k.text) record(TEXT, 0, k.text);
  record(KEY_UP, k.sym, k.scan, 0, k.mods);
}
const ctrl = letter => press(keyEvent(letter, { ctrlKey: true }));

// The first present after t, or after the call
async function presentAfter(what, t = now(), ms = 10000) {
  const f = await waitFor(what, () => S.frames.find(f => f.t > t), ms);
  return f.t - t;
}

// Types text one key at a time, each until the frame shows it
async function type(text) {
  for (const ch of text) {
    const t = now();
    press(keyEvent(ch));
    timing('keystroke -> present', await presentAfter('the present of ' + JSON.stringify(ch), t));
  }
}

// ---- the probe

let lastProbe = null;
function readProbe() {
  try {
    const p = JSON.parse(S.drv.FS.readFile('/pharo/probe.json', { encoding: 'utf8' }));
    if (p && p.seq) lastProbe = p;
  } catch (e) { /* being written, or not yet */ }
  return lastProbe;
}
// A probe written after the call that pred accepts
async function probe(what, pred, ms = 30000) {
  const after = (readProbe() || { seq: 0 }).seq;
  return waitFor(what, () => {
    const p = readProbe();
    return p && p.seq > after && pred(p) && p;
  }, ms);
}
const menuItem = (p, label) => {
  for (const m of p.menus) for (const i of m.items) if (i.label === label) return i;
  return null;
};

async function check(name, f) {
  try {
    await f();
    passes++;
    console.log('ok - ' + name);
  } catch (e) {
    failures++;
    console.log('not ok - ' + name + '\n  ' + String((e && e.message) || e));
    if (lastProbe) console.log('  probe: ' + JSON.stringify(lastProbe).slice(0, 1500));
    if (S && S.err) console.log('  stderr: ' + S.err.slice(-800));
  }
}
function assert(c, msg) { if (!c) throw new Error('assertion failed: ' + msg); }
const intersects = (r, b) => r[0] < b[2] && r[0] + r[2] > b[0] && r[1] < b[3] && r[1] + r[3] > b[1];

const median = a => { const s = a.slice().sort((x, y) => x - y); return s[Math.floor((s.length - 1) / 2)]; };

(async () => {
  let started = false;

  await check('1 display-worker.js on a canvas, with no VM: dirty rectangles, sizes, memory growth, a full ring', async () => {
    // an OffscreenCanvas and ImageData as a browser has them, and a module
    const calls = [], posted = [];
    globalThis.ImageData = class { constructor(data, width, height) { Object.assign(this, { data, width, height }); calls.push(['ImageData', width, height]); } };
    const ctx = { putImageData: (img, ...at) => calls.push(['put', img, ...at]) };
    const canvas = { width: 300, height: 150, getContext: (type, o) => { calls.push(['getContext', type, JSON.stringify(o)]); return ctx; } };
    const memory = new WebAssembly.Memory({ initial: 1, maximum: 4 });
    let full = false, kicks = 0, extent = null, clip = null;
    const pushed = [];
    const module = {
      get HEAPU8() { return new Uint8Array(memory.buffer); },
      get HEAP32() { return new Int32Array(memory.buffer); },
      _webdisplay_push_event: (...e) => { if (full) return 0; pushed.push(e); return 1; },
      _webdisplay_set_extent: (w, h) => { extent = [w, h]; },
      _webdisplay_clipboard_buffer: n => 16384,
      _webdisplay_clipboard_commit: n => { clip = new TextDecoder().decode(new Uint8Array(memory.buffer, 16384, n)); },
    };
    const vm = { module, dead: false, kick() { kicks++; } };
    try {
      const d = PharoDisplay.create({ canvas }, (m, transfer) => posted.push({ m, transfer }));
      assert(JSON.stringify(calls.shift()) === '["getContext","2d","{\\"alpha\\":false}"]', 'a 2d context without alpha');
      d.present(1024, 40, 30, 8192, 1);
      assert(!calls.length && !posted.length, 'no VM yet, so no memory: nothing painted');
      d.handle({ kind: 'attach' }, vm);
      assert(!kicks && !pushed.length && !extent && !calls.length && !posted.length, 'attach tells the VM only');
      d.handle({ type: 'display', kind: 'resize', width: 40, height: 30 }, vm);
      assert(String(extent) === '40,30' && String(pushed[0].slice(2)) === '0,0,40,0,30,0' && pushed[0][0] === 8 && kicks === 1,
             'the extent, then a resize record, then a kick: ' + JSON.stringify(pushed));
      const rects = (...r) => new Int32Array(memory.buffer).set(r, 8192 / 4);
      const puts = () => calls.filter(c => c[0] === 'put').map(c => c.slice(2).join(','));
      const images = () => calls.filter(c => c[0] === 'ImageData').length;
      rects(0, 0, 40, 30);
      d.present(1024, 40, 30, 8192, 1);
      assert(canvas.width === 40 && canvas.height === 30, 'the canvas takes the size of the frame');
      assert(images() === 1 && String(puts()) === '0,0', 'a new size is painted whole: ' + puts());
      const img = calls.find(c => c[0] === 'put')[1];
      assert(img.data.buffer === memory.buffer && img.data.byteOffset === 1024 && img.data.length === 40 * 30 * 4, 'an ImageData over the frame');
      calls.length = 0;
      rects(1, 2, 3, 4, 5, 6, 7, 8);
      d.present(1024, 40, 30, 8192, 2);
      assert(!images() && puts().join(' ') === '0,0,1,2,3,4 0,0,5,6,7,8', 'the dirty rectangles, on the same ImageData: ' + puts());
      calls.length = 0;
      memory.grow(1);
      d.present(1024, 40, 30, 8192, 1);
      const grown = calls.find(c => c[0] === 'put')[1];
      assert(images() === 1 && grown.data.buffer === memory.buffer && grown.data.length === 4800, 'a new ImageData after the memory grew');
      calls.length = 0;
      d.present(2048, 40, 30, 8192, 1);
      assert(images() === 1 && calls.find(c => c[0] === 'put')[1].data.byteOffset === 2048, 'and after the frame moved');
      calls.length = 0;
      d.present(2048, 20, 10, 8192, 1);
      assert(canvas.width === 20 && canvas.height === 10 && images() === 1 && String(puts()) === '0,0', 'a smaller frame, painted whole');
      const frames = posted.filter(p => p.m.kind === 'frame').map(p => p.m.frame);
      assert(String(frames) === '1,2,3,4,5' && d.stats.presents === 5, 'a frame message per present: ' + frames);
      const rgba = new Uint8ClampedArray(16);
      d.setCursor(rgba, 2, 2, 1, 0);
      const cursor = posted[posted.length - 1];
      assert(cursor.m.kind === 'cursor' && cursor.m.rgba === rgba.buffer && cursor.transfer[0] === rgba.buffer &&
             cursor.m.hotX === 1 && cursor.m.hotY === 0, 'the cursor, its pixels transferred');
      // the ring refuses: records wait, in order, the moves merging
      full = true;
      pushed.length = 0;
      const ev = (...r) => d.handle({ type: 'display', kind: 'event', event: r }, vm);
      ev(1, 0, 10, 10, 0, 0, 0, 0);
      ev(1, 0, 11, 11, 0, 0, 0, 0);
      ev(2, 0, 11, 11, 1, 256, 0, 0);
      ev(1, 0, 12, 12, 0, 256, 0, 0);
      ev(1, 0, 13, 13, 0, 256, 0, 0);
      assert(d.backlog === 3 && d.stats.refused > 0 && !pushed.length, 'three waiting: ' + d.backlog);
      full = false;
      const k = kicks;
      await sleep(100);
      assert(d.backlog === 0 && pushed.map(e => e.slice(0, 3).join(',')).join(' ') === '1,0,11 2,0,11 1,0,13' && kicks > k,
             'pushed again later, then a kick: ' + JSON.stringify(pushed));
      d.handle({ type: 'display', kind: 'clipboard', text: 'héllo ✓' }, vm);
      assert(clip === 'héllo ✓', 'the paste in the clipboard buffer, committed: ' + clip);
      vm.dead = true;
      pushed.length = 0;
      ev(1, 0, 1, 1, 0, 0, 0, 0);
      assert(!pushed.length, 'nothing for a dead VM');
      // attach alone is enough for present
      const other = { width: 0, height: 0, getContext: () => ctx };
      const d2 = PharoDisplay.create({ canvas: other }, () => {});
      d2.handle({ kind: 'attach' }, Object.assign({}, vm, { dead: false }));
      rects(0, 0, 4, 2);
      d2.present(1024, 4, 2, 8192, 1);
      assert(other.width === 4 && other.height === 2 && d2.stats.presents === 1, 'a present after attach paints');
    } finally {
      delete globalThis.ImageData;
    }
  });


  await check('2 the world\'s first frame has the size of the canvas, 1024x768, within 60 s', async () => {
    await session();
    assert(S.drv, 'start answered null: ' + JSON.stringify(S.crash));
    const first = await waitFor('a first frame', () => S.frames[0], 60000);
    timing('first frame (from start)', first.t - S.t0);
    assert(first.width === WIDTH && first.height === HEIGHT,
           `the first frame is ${first.width}x${first.height}, not the canvas's ${WIDTH}x${HEIGHT}`);
    const open = S.msgs.find(m => m.kind === 'displayOpen');
    assert(open && open.title === '/pharo/Pharo.image' && open.width === WIDTH && open.height === HEIGHT,
           'displayOpen ' + JSON.stringify(open));
    // the first frame of a size is painted whole
    const frame = S.display.frame;
    assert(frame.width === WIDTH && frame.height === HEIGHT && frame.data.length === WIDTH * HEIGHT * 4, 'the framebuffer');
    const counts = new Map();
    const px = new Uint32Array(frame.data.buffer, frame.data.byteOffset, WIDTH * HEIGHT);
    let opaque = true;
    for (const v of px) { counts.set(v, (counts.get(v) || 0) + 1); if (v >>> 24 !== 255) opaque = false; }
    const background = Math.max(...counts.values()) / px.length;
    console.log(`#   ${counts.size} colours; the commonest covers ${(100 * background).toFixed(1)}%`);
    assert(opaque, 'every pixel opaque');
    assert(1 - background > 0.3, `more than 30% of the pixels not the background colour (${(100 * (1 - background)).toFixed(1)}%)`);
    started = true;
  });
  if (!started) { console.log('cannot continue'); process.exit(1); }

  let p;
  await check('3 the probe: OSWebDriver, a World of the canvas size, a menubar', async () => {
    p = await probe('the World at 1024x768', p => p.world[0] === WIDTH && p.world[1] === HEIGHT);
    assert(p.renderer === 'OSWorldRenderer' && p.driver === 'OSWebDriver', `renderer ${p.renderer}, driver ${p.driver}`);
    assert(p.display && p.display[0] === WIDTH && p.display[1] === HEIGHT, 'display ' + p.display);
    assert(p.menubar.some(i => i.label === 'Browse'), 'menubar ' + JSON.stringify(p.menubar));
    assert(!p.error, p.error);
    assert(S.msgs.some(m => m.kind === 'cursor' && m.rgba.byteLength === m.width * m.height * 4), 'a cursor, as RGBA');
  });

  await check(`4 the fonts of the world: ${manifest.fonts === 'freetype' ? 'FreeType, with glyphs for \u03bb\u2192' : 'bitmap fonts'} (manifest.fonts ${manifest.fonts})`, async () => {
    // the fonts that the preparation set up, which the probe says draw
    const freetype = manifest.fonts === 'freetype';
    assert(p.fonts, 'the probe says no fonts: ' + JSON.stringify(p).slice(0, 300));
    for (const which of ['default', 'code', 'menu', 'windowTitle', 'menubar']) {
      const cls = p.fonts[which];
      assert(freetype ? cls === 'FreeTypeFont' : cls && cls !== 'FreeTypeFont', `the ${which} font is drawn by ${cls}`);
    }
    if (freetype) assert(p.fonts.family === 'Source Sans Pro', 'the family of the default font: ' + p.fonts.family);
    assert(p.glyphs === freetype, `glyphs for \u03bb\u2192: ${p.glyphs}`);
  });

  await check('5 a click on Browse in the menubar opens its menu, which the next frames paint', async () => {
    const browse = p.menubar.find(i => i.label === 'Browse');
    const t = now();
    await click(...center(browse.bounds));
    p = await probe('the Browse menu', p => menuItem(p, 'Playground'));
    const menu = p.menus.find(m => m.items.some(i => i.label === 'Playground'));
    const bounds = menu.items.reduce((b, i) => [Math.min(b[0], i.bounds[0]), Math.min(b[1], i.bounds[1]),
                                                Math.max(b[2], i.bounds[2]), Math.max(b[3], i.bounds[3])],
                                     [Infinity, Infinity, -Infinity, -Infinity]);
    const painted = S.frames.find(f => f.t > t && f.rects.some(r => intersects(r, bounds)));
    assert(painted, 'a frame painting the menu at ' + bounds);
    timing('click -> present', S.frames.find(f => f.t > t).t - t);
    timing('click -> frame of the menu', painted.t - t);
  });

  await check('6 its Playground item opens a Playground; typing 3 + 4, then Ctrl+P prints 7', async () => {
    await click(...center(menuItem(p, 'Playground').bounds));
    p = await probe('a Playground', p => p.playground && p.menus.length === 0, 30000);
    await click(...center(p.playground.bounds));
    await sleep(300);
    await type('3 + 4');
    p = await probe('the typed text', p => p.playground.text === '3 + 4');
    const t = now();
    ctrl('p');
    p = await probe('7 printed', p => p.printed.includes('7'));
    timing('Ctrl+P -> probe', now() - t);
    assert(p.playground.text === '3 + 4', 'the Playground keeps its text: ' + JSON.stringify(p.playground.text));
  });

  await check('7 the clipboard: Ctrl+C reaches the page, a paste of the page reaches Ctrl+V', async () => {
    await click(...center(p.playground.bounds));          // closes the popover
    await probe('no popover', p => p.printed.length === 0);
    const before = S.msgs.length;
    ctrl('a');
    ctrl('c');
    await waitFor('clipboardSet', () => S.msgs.slice(before).find(m => m.kind === 'clipboardSet' && m.text === '3 + 4'));
    page('clipboard', { text: '6 * 7 "✓"' });
    ctrl('a');
    ctrl('v');
    p = await probe('the pasted text', p => p.playground.text === '6 * 7 "✓"');
    ctrl('a');
    ctrl('p');
    p = await probe('42 printed', p => p.printed.includes('42'));
    // bytes that are not UTF-8, as the page never sends them: a lone
    // continuation byte, and a sequence that the end cuts
    await click(...center(p.playground.bounds));          // closes the popover
    p = await probe('no popover', p => p.printed.length === 0);
    const M = S.drv.module, bad = [104, 169, 105, 195];
    const at = Number(M._webdisplay_clipboard_buffer(bad.length));
    M.HEAPU8.set(bad, at);
    M._webdisplay_clipboard_commit(bad.length);
    ctrl('a');
    ctrl('v');
    p = await probe('the bad bytes pasted, as question marks', p => p.playground.text === 'h?i?');
    assert(!p.debugger, 'no debugger');
  });

  await check('8 a resize to 800x600: a frame of 800x600, and the World follows', async () => {
    const t = now();
    page('resize', { width: 800, height: 600 });
    const f = await waitFor('a frame of 800x600', () => S.frames.find(f => f.t > t && f.width === 800 && f.height === 600));
    timing('resize -> frame', f.t - t);
    assert(S.display.frame.width === 800 && S.display.frame.height === 600, 'the framebuffer follows');
    p = await probe('a World of 800x600', p => p.world[0] === 800 && p.world[1] === 600);
    page('resize', { width: WIDTH, height: HEIGHT });
    p = await probe('a World of 1024x768 again', p => p.world[0] === WIDTH && p.world[1] === HEIGHT);
  });

  await check('9 a burst of 400 mouse moves: the ring overflows, the moves wait and merge, the last arrives', async () => {
    const before = S.display.stats.refused;
    for (let i = 0; i < 400; i++) move(100 + i, 200 + (i % 50));
    assert(S.display.stats.refused > before, 'the ring refused some');
    assert(S.display.backlog === 1, 'the display keeps them, merged: ' + S.display.backlog);
    p = await probe('the hand at the last move', p => p.hand[0] === 499 && p.hand[1] === 249);
    assert(S.display.backlog === 0, 'nothing left waiting');
  });

  await check('10 typing 56 more keys, and Ctrl+P prints what they say', async () => {
    await click(...center(p.playground.bounds));          // closes the popover
    p = await probe('no popover', p => p.printed.length === 0);
    ctrl('a');
    const text = 'Smalltalk version size + 1000 factorial printString size';
    await type(text);
    p = await probe('the typed text', p => p.playground.text === text);
    ctrl('p');
    p = await probe('a number printed', p => p.printed.some(s => /^\d+$/.test(s)));
  });

  await check('11 Stop (vm_interrupt) during a busy loop of the UI process opens a debugger', async () => {
    // pasted: Rubric pairs the brackets typed
    const loop = '[ true ] whileTrue';
    await click(...center(p.playground.bounds));
    p = await probe('no popover', p => p.printed.length === 0);
    page('clipboard', { text: loop });
    ctrl('a');
    ctrl('v');
    p = await probe('the busy loop in the Playground', p => p.playground.text === loop);
    ctrl('d');
    await sleep(1500);
    const recent = S.states.filter(s => s.t > now() - 1000);
    assert(recent.length && recent.every(s => s.st === BUSY), 'the VM is busy: ' + recent.map(s => s.st).slice(-10));
    const t = now();
    S.errAtStop = S.err.length;
    assert(S.drv.interrupt() === true, 'the image registered its interrupt semaphore');
    p = await probe('a debugger', p => p.debugger, 30000);
    timing('Stop -> debugger (probe)', now() - t);
    S.errAfterStop = S.err.length;
    await waitFor('the VM idle again', () => S.states.length && S.states[S.states.length - 1].st !== BUSY, 10000);
  });

  await check('12 a save request snapshots the image: HOST_IMAGE_SAVED, and the world goes on', async () => {
    const t = now(), before = S.host.length;
    record(SAVE);
    const saved = await waitFor('HOST_IMAGE_SAVED', () => S.host.slice(before).find(h => h.kind === HOST_IMAGE_SAVED), 120000);
    timing('save request -> HOST_IMAGE_SAVED', saved.t - t);
    assert(saved.text === '/pharo/Pharo.image', 'the path ' + saved.text);
    const size = S.drv.FS.stat('/pharo/Pharo.image').size;
    assert(size > 50e6, 'an image of ' + size + ' bytes');
    // the world takes input after the save: right click on the desktop
    p = await probe('a probe after the save', p => p.seq);
    const desk = [Math.round(WIDTH * 0.9), Math.round(HEIGHT * 0.8)];
    await click(...desk, 3);
    p = await probe('the world menu', p => p.menus.length > 0);
    press(keyEvent('Escape'));
    p = await probe('the world menu closed by Escape', p => p.menus.length === 0);
  });

  await check('13 the keystroke-to-present median is under 100 ms', async () => {
    const k = timings['keystroke -> present'] || [];
    assert(k.length >= 60, k.length + ' keystrokes');
    assert(median(k) < 100, 'median ' + median(k).toFixed(1) + ' ms');
  });

  await check('14 nothing on the image\'s stderr but the stack of the process Stop interrupted', async () => {
    const before = S.err.slice(0, S.errAtStop), stop = S.err.slice(S.errAtStop, S.errAfterStop);
    const after = S.err.slice(S.errAfterStop);
    assert(S.errAtStop !== undefined && !before, 'before Stop: ' + JSON.stringify(before.slice(0, 800)));
    assert(/UndefinedObject>>DoIt/.test(stop), 'the interrupted DoIt: ' + JSON.stringify(stop.slice(0, 300)));
    assert(!after, 'after Stop: ' + JSON.stringify(after.slice(0, 800)));
    assert(!S.exit && !S.crash, 'the VM runs');
  });

  await check('15 the runtime reported no engine error, nor an exception of the display', async () => {
    const bad = S.diag.split('\n').filter(line => ENGINE_ERRORS.test(line));
    assert(!bad.length, bad.slice(0, 5).join(' | '));
  });

  console.log('# timing (ms)                          median      max    n');
  for (const [what, a] of Object.entries(timings)) {
    console.log('# ' + what.padEnd(36) + median(a).toFixed(1).padStart(8) + Math.max(...a).toFixed(1).padStart(9) +
                String(a.length).padStart(5));
  }
  console.log(`# ${passes + failures} cases, ${passes} passed, ${failures} failed`);
  process.exit(failures ? 1 : 0);
})();
