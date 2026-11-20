// pango.spec.mjs - Pango and Poppler in a browser (WASM_PANGO, WASM_POPPLER)
//
// usage: node pango.spec.mjs WEB_DIR
//
// A plain node script, as page.spec.mjs (tests/wasm/lib/pw.mjs serves
// WEB_DIR and drives the Console page in each browser of BROWSERS), for a
// site built with WASM_PANGO and WASM_POPPLER whose catalog has an image
// with libpangocairo.st and libpoppler.st loaded: the batteries image of
// docs/Batteries.md (PANGO_IMAGE_ID, default 'batteries').  The Console
// starts it fresh (?fresh=<id>), and its REPL evaluates through the
// bindings, in the web VM of the page:
//
//   - PangoLibrary versionString, the pinned Pango;
//   - a PangoLayout of markup with a bold word, wider than the same text
//     plain (the bold DejaVu Sans of the VM's fonts);
//   - PopplerLibrary versionString, and a one-page PDF written by cairo's
//     PDF surface (AthensCairoPDFSurface), read back by PopplerDocument:
//     one page, whose text holds what was drawn.
//
// Skipped (status 0, a 'skip' line) when the manifest has no
// libpango-1.0.so.0 placeholder, or its catalog no such image.  Exits with
// status 1 if any check fails or the page logs an error.

import { run } from './lib/pw.mjs';

const id = process.env.PANGO_IMAGE_ID || 'batteries';

await run(async t => {
  const { page, check, manifest } = t;
  if (!(manifest.libraries || []).includes('libpango-1.0.so.0')) {
    console.log('skip: the site has no Pango (WASM_PANGO is off)');
    return;
  }
  if (!(manifest.images || []).some(i => i.id === id)) {
    console.log(`skip: the catalog has no image '${id}' (PANGO_IMAGE_ID)`);
    return;
  }
  const term = () => page.$eval('#term', e => e.textContent);
  let markAt = 0;
  const waitStatus = (st, timeout) =>
    page.waitForFunction(s => document.getElementById('status').dataset.state === s, st, { timeout });
  const waitSince = (re, timeout) =>
    page.waitForFunction(([src, fl, at]) => new RegExp(src, fl).test(document.getElementById('term').textContent.slice(at)),
                         [re.source, re.flags, markAt], { timeout });
  async function evalTo(text, re, timeout = 120000) {
    markAt = (await term()).length;
    await page.fill('#line', text);
    await page.press('#line', 'Enter');
    await waitSince(re, timeout);
    await waitStatus('waiting', timeout);
  }

  await page.goto(t.base + 'index.html?fresh=' + id);
  await check(`the Console starts the image '${id}'`, async () => {
    await waitStatus('waiting', 180000);
    await waitSince(/(^|\n)st> $/, 180000);
  });
  await check('PangoLibrary versionString', async () => {
    await evalTo('PangoLibrary versionString', /'1\.58\.2'/);
  });
  await check('a bold word makes a PangoLayout wider', async () => {
    await evalTo("(PangoLayout measuringDo: [ :l | l markup: '<b>Hello</b> Pango'. l logicalRectangle width ]) > " +
                 "(PangoLayout measuringDo: [ :l | l markup: 'Hello Pango'. l logicalRectangle width ])", /\btrue\b/);
  });
  await check('PopplerLibrary versionString', async () => {
    await evalTo('PopplerLibrary versionString', /'26\.11\.0'/);
  });
  await check('a PDF of cairo, read by Poppler', async () => {
    await evalTo("| file surface document text | file := FileLocator temp / 'pango-spec.pdf'. " +
                 "surface := AthensCairoPDFSurface extent: 200 @ 100 fileName: file fullName. " +
                 "surface drawDuring: [ :c | c setPaint: Color black. c setFont: (LogicalFont familyName: 'Source Sans Pro' pointSize: 20). " +
                 "c pathTransform translateX: 10 Y: 50. c drawString: 'Pharo PDF' ]. surface finish. " +
                 "document := PopplerDocument fromFile: file. " +
                 "text := document pages first text. { document pages size. (text includesSubstring: 'Pharo') }",
                 /\{1\. true\}|#\(1 true\)/);
  });
});
