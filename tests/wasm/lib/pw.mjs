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

import { createRequire } from 'node:module';
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';

const require = createRequire(import.meta.url);
const { handler } = await import(new URL('../../../packaging/emscripten/tools/serve.mjs', import.meta.url));

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
