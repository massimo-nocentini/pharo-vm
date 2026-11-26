// cairo-shots.mjs - the example notebook of a site, Run all, then screenshots
// of its 'Notebook cairo:' cell (the Roassal drawing as SVG), in the light and
// the dark scheme, as made for docs/WebAssembly.md (the Notebook's cairo:)
//
//   [OUT=<dir>] PLAYWRIGHT_MODULE=... node tools/sessions/screenshots/cairo-shots.mjs WEB_DIR
//
// It checks, as a spec does (tests/wasm/lib/pw.mjs: BROWSERS, PLAYWRIGHT_*), that
// the cell is ok and shows an SVG with glyphs, and writes cairo-cell-<scheme>.png
// and notebook-<scheme>.png into OUT (default: the current directory).
const { run } = await import(new URL('../../../tests/wasm/lib/pw.mjs', import.meta.url));
const out = process.env.OUT || process.cwd();
await run(async (t) => {
  for (const scheme of ['light', 'dark']) {
    const ctx = await t.browser.newContext({ colorScheme: scheme, viewport: { width: 1100, height: 900 } });
    const page = await ctx.newPage();
    t.watch(page);
    await page.goto(t.base + 'index.html');
    await page.click('#tab-notebook');
    await page.waitForFunction(() => document.getElementById('nb-status').dataset.state === 'ready', null, { timeout: 180000 });
    await page.click('#nb-run-all');
    await page.waitForFunction(() => {
      const l = [...document.querySelectorAll('#nb-cells > li[data-type="code"]')];
      return l.length && l.every(e => /^(ok|error|cancelled|interrupted)$/.test(e.dataset.status));
    }, null, { timeout: 180000 });
    const cell = page.locator('#nb-cells > li[data-type="code"]', { hasText: 'Notebook cairo:' });
    await t.check(scheme + ': the cairo: cell is ok and shows an SVG with glyphs', async () => {
      t.assert(await cell.getAttribute('data-status') === 'ok', 'status ' + await cell.getAttribute('data-status'));
      const info = await cell.evaluate(li => {
        const svg = li.querySelector('.nb-rich svg');
        return svg && { w: svg.getAttribute('width'), h: svg.getAttribute('height'), uses: svg.querySelectorAll('use').length,
                        paths: svg.querySelectorAll('path').length, box: svg.getBoundingClientRect().width };
      });
      console.log('  # ' + JSON.stringify(info));
      t.assert(info && info.uses > 50 && info.paths > 20 && info.box > 400, 'svg ' + JSON.stringify(info));
    });
    await cell.scrollIntoViewIfNeeded();
    await cell.screenshot({ path: `${out}/cairo-cell-${scheme}.png` });
    await page.evaluate(() => {
      const li = [...document.querySelectorAll('#nb-cells > li[data-type="code"]')].find(e => e.textContent.includes('Notebook cairo:'));
      li.previousElementSibling.scrollIntoView({ block: 'start' });
    });
    await page.screenshot({ path: `${out}/notebook-${scheme}.png` });
    await ctx.close();
  }
});
