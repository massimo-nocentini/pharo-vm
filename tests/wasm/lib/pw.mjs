// pw.mjs - the shared runner of the Playwright specs of the web pages
//
//   import { run } from './lib/pw.mjs';
//   await run(async (t) => { ... checks ... });
//
// A spec is a plain node script (no @playwright/test), run as
//
//   node tests/wasm/<name>.spec.mjs WEB_DIR
//
// It is never a build dependency: `make wasm-check-browser' runs the specs
// when PLAYWRIGHT_MODULE is set.  run() serves WEB_DIR over HTTP with
// packaging/emscripten/tools/serve.mjs (no COOP/COEP headers, like any
// static host), then, for each browser named by BROWSERS, launches it, opens
// a context and a page, and calls the spec with t:
//
//   t.name             the browser: chromium, firefox, ...
//   t.browser, t.context, t.page
//   t.base             the URL of WEB_DIR, ending in /
//   t.webDir, t.manifest   WEB_DIR and its manifest.json
//   t.check(name, f)   runs the check f, printing 'ok - <browser>: name' or
//                      'not ok - ...' with the error
//   t.assert(c, msg)
//   t.requests         the paths that the pages and their workers requested
//                      so far, in order, as a route of the context sees them
//                      (which turns the HTTP cache off: every one reaches
//                      the server)
//   t.errors           what went wrong in the browser so far: console errors
//                      of the pages and their workers, uncaught exceptions
//                      and failed requests
//   t.watch(page)      records the errors of another page too
//   t.shots            the SHOTS directory, or null
//   t.shot(file)       the path of a screenshot <browser>-<file> in it
//
// After the spec, a last check per browser says that there were no errors.
// The process exits with status 1 if any check failed.  Environment:
//
//   PLAYWRIGHT_MODULE  the playwright package to use, as a path (e.g.
//                      /some/dir/node_modules/playwright); default: resolve
//                      "playwright" from here
//   BROWSERS           a comma separated list, default "chromium"; e.g.
//                      BROWSERS=chromium,firefox
//   SHOTS              a directory for the screenshots of the specs, or 1 for
//                      TEST_DIR/shots (default WEB_DIR/../tests-run/shots)
//
// The usual Playwright variables apply (PLAYWRIGHT_BROWSERS_PATH, ...).
//
// decodePNG(buffer) answers {width, height, data} with data the RGBA bytes
// of a PNG, such as a screenshot of a canvas.
//
// dropFiles(page, paths) drops the files of paths on the page, as a user
// dragging them from the desktop: it serves them to the page, which makes
// them Files of a DataTransfer, and dispatches dragenter, dragover and drop
// on its body.  Answers {shown, took, hidden}: whether the page showed its
// #drop element after the dragenter, took the files (prevented the default
// of the drop) and hid that element again.

import { createRequire } from 'node:module';
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';
import zlib from 'node:zlib';

const require = createRequire(import.meta.url);
const { handler } = await import(new URL('../../../packaging/emscripten/tools/serve.mjs', import.meta.url));

export function decodePNG(buffer) {
  const b = Buffer.from(buffer);
  if (b.readUInt32BE(0) !== 0x89504e47) throw new Error('not a PNG');
  let at = 8, width = 0, height = 0, depth = 0, type = 0, interlace = 0;
  const idat = [];
  while (at < b.length) {
    const length = b.readUInt32BE(at), kind = b.toString('latin1', at + 4, at + 8), data = b.subarray(at + 8, at + 8 + length);
    if (kind === 'IHDR') {
      width = data.readUInt32BE(0);
      height = data.readUInt32BE(4);
      depth = data[8];
      type = data[9];
      interlace = data[12];
    } else if (kind === 'IDAT') idat.push(data);
    else if (kind === 'IEND') break;
    at += 12 + length;
  }
  const channels = { 0: 1, 2: 3, 4: 2, 6: 4 }[type];
  if (depth !== 8 || !channels || interlace) throw new Error(`unsupported PNG (depth ${depth}, type ${type}, interlace ${interlace})`);
  const raw = zlib.inflateSync(Buffer.concat(idat)), stride = width * channels;
  const pixels = Buffer.alloc(stride * height);
  for (let y = 0; y < height; y++) {
    const filter = raw[y * (stride + 1)], row = raw.subarray(y * (stride + 1) + 1, (y + 1) * (stride + 1));
    const out = y * stride, up = out - stride;
    for (let x = 0; x < stride; x++) {
      const a = x >= channels ? pixels[out + x - channels] : 0;
      const u = y ? pixels[up + x] : 0;
      const c = x >= channels && y ? pixels[up + x - channels] : 0;
      let v = row[x];
      if (filter === 1) v += a;
      else if (filter === 2) v += u;
      else if (filter === 3) v += (a + u) >> 1;
      else if (filter === 4) {
        const p = a + u - c, pa = Math.abs(p - a), pb = Math.abs(p - u), pc = Math.abs(p - c);
        v += pa <= pb && pa <= pc ? a : pb <= pc ? u : c;
      }
      pixels[out + x] = v & 255;
    }
  }
  const data = Buffer.alloc(width * height * 4);
  for (let i = 0, j = 0; i < width * height; i++, j += channels) {
    const g = pixels[j];
    data[i * 4] = channels >= 3 ? pixels[j] : g;
    data[i * 4 + 1] = channels >= 3 ? pixels[j + 1] : g;
    data[i * 4 + 2] = channels >= 3 ? pixels[j + 2] : g;
    data[i * 4 + 3] = channels === 4 ? pixels[j + 3] : channels === 2 ? pixels[j + 1] : 255;
  }
  return { width, height, data };
}

let drops = 0;
export async function dropFiles(page, paths) {
  const urls = [];
  for (const file of paths) {
    const url = '/__drop__/' + (++drops) + '/' + encodeURIComponent(path.basename(file));
    await page.route('**' + url, route => route.fulfill({ path: file, contentType: 'application/octet-stream' }));
    urls.push(url);
  }
  return page.evaluate(async urls => {
    const data = new DataTransfer();
    for (const url of urls) {
      const blob = await (await fetch(url)).blob();
      data.items.add(new File([blob], decodeURIComponent(url.slice(url.lastIndexOf('/') + 1))));
    }
    const fire = type => document.body.dispatchEvent(new DragEvent(type, { dataTransfer: data, bubbles: true, cancelable: true }));
    fire('dragenter');
    const drop = document.getElementById('drop'), shown = !!drop && !drop.hidden;
    fire('dragover');
    const took = !fire('drop');
    return { shown, took, hidden: !drop || drop.hidden };
  }, urls);
}

export async function run(spec) {
  const pw = require(process.env.PLAYWRIGHT_MODULE || 'playwright');
  const webDir = path.resolve(process.argv[2] || 'build-wasm/web');
  const manifest = JSON.parse(fs.readFileSync(path.join(webDir, 'manifest.json'), 'utf8'));
  const browsers = (process.env.BROWSERS || 'chromium').split(',').map(s => s.trim()).filter(Boolean);
  let shots = process.env.SHOTS || null;
  if (shots === '1' || shots === 'yes')
    shots = path.join(process.env.TEST_DIR || path.join(webDir, '..', 'tests-run'), 'shots');
  if (shots) fs.mkdirSync(shots = path.resolve(shots), { recursive: true });

  const server = http.createServer(handler(webDir));
  await new Promise(r => server.listen(0, '127.0.0.1', r));
  const base = 'http://127.0.0.1:' + server.address().port + '/';

  let failures = 0, passes = 0;
  for (const name of browsers) {
    const errors = [];
    const watch = page => {
      page.on('console', m => { if (m.type() === 'error') errors.push('console: ' + m.text()); });
      page.on('pageerror', e => errors.push('pageerror: ' + e.message));
      page.on('worker', w => w.on('console', m => {
        if (m.type() === 'error') errors.push('worker console: ' + m.text());
      }));
      page.on('requestfailed', r => errors.push('request failed: ' + r.url() + ' (' + (r.failure() || {}).errorText + ')'));
    };
    const check = async (what, f) => {
      try {
        await f();
        passes++;
        console.log(`ok - ${name}: ${what}`);
      } catch (e) {
        failures++;
        console.log(`not ok - ${name}: ${what}\n  ` + String((e && e.message) || e).split('\n').join('\n  '));
      }
    };
    let browser = null;
    try {
      browser = await pw[name].launch();
      const context = await browser.newContext({ viewport: { width: 1280, height: 800 }, acceptDownloads: true });
      const requests = [];
      await context.route('**/*', route => {
        requests.push(new URL(route.request().url()).pathname);
        route.continue();
      });
      const page = await context.newPage();
      watch(page);
      await spec({
        name, browser, context, page, base, webDir, manifest, requests, errors, watch, check, shots,
        assert(c, msg) { if (!c) throw new Error('assertion failed: ' + msg); },
        shot: file => shots ? path.join(shots, `${name}-${file}`) : null,
      });
      await check('zero console, page and worker errors', async () => {
        if (errors.length) throw new Error(errors.join('\n'));
      });
    } catch (e) {
      failures++;
      console.log(`not ok - ${name}: ` + ((e && e.stack) || e));
    } finally {
      if (browser) await browser.close();
    }
  }
  server.close();
  if (shots) console.log('# screenshots in ' + shots);
  console.log(`# ${passes} passed, ${failures} failed`);
  process.exit(failures ? 1 : 0);
}
