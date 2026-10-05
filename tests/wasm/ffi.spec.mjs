// ffi.spec.mjs - the FFI of the web VM in a browser, through the Console page
//
// usage: node ffi.spec.mjs WEB_DIR   (see tests/wasm/lib/pw.mjs)
//
// For a build with the FFI (manifest.json says "ffi": true; make
// wasm-check-browser skips this spec otherwise).  The Console evaluates
// uFFI callouts into the C library of the registry (LibC, libc.so.6): a
// callback (qsort, whose closure the worker compiles as a small WebAssembly
// module), the environment through setenv, and a declaration that does not
// match its function (strlen declared double, where it answers a size_t),
// which must fail as a primitive, say why in the worker's console, and leave
// the VM running.  With the last check of pw.mjs (no console errors: the
// worker says why as a warning), 5 checks for each browser.

import { run } from './lib/pw.mjs';

await run(async t => {
  const { page, check, assert } = t;
  const term = () => page.$eval('#term', e => e.textContent);
  let markAt = 0;
  const waitStatus = (st, timeout = 30000) =>
    page.waitForFunction(s => document.getElementById('status').dataset.state === s, st, { timeout });
  async function evalTo(text, re, timeout = 30000) {
    markAt = (await term()).length;
    await page.fill('#line', text);
    await page.press('#line', 'Enter');
    await page.waitForFunction(([src, at]) => new RegExp(src).test(document.getElementById('term').textContent.slice(at)),
                               [re.source, markAt], { timeout });
    await waitStatus('waiting', timeout);
    return (await term()).slice(markAt);
  }
  const workerLog = [];
  page.on('worker', w => w.on('console', m => workerLog.push(m.text())));

  await page.goto(t.base + 'index.html');
  await waitStatus('waiting', 60000);

  await check('the image uses the TFFI backend', async () => {
    assert(t.manifest.ffi === true, 'manifest.json says "ffi": true, not ' + JSON.stringify(t.manifest.ffi));
    await evalTo('FFIBackend current class name', /#TFFIBackend/);
  });
  await check('a qsort with a Smalltalk callback', async () => {
    await evalTo("((Object << #WasmFFIPage) package: 'WasmFFIPage') install", /WasmFFIPage/);
    await evalTo("WasmFFIPage compile: 'qs: b n: n s: s with: c ^ self ffiCall: #(void qsort(ByteArray b, size_t n, size_t s, FFICallback c)) library: LibC'", /#qs:n:s:with:/);
    const out = await evalTo("| a cb | a := ByteArray new: 24. #(5 3 9 1 7 2) doWithIndex: [:e :i | a at: i * 4 - 3 put: e]. " +
      "cb := FFICallback signature: #(int (void *x, void *y)) block: [:x :y | (x signedLongAt: 1) - (y signedLongAt: 1)]. " +
      "WasmFFIPage new qs: a n: 6 s: 4 with: cb. (1 to: 6) collect: [:i | a at: i * 4 - 3]", /#\(1 2 3 5 7 9\)|Error/);
    assert(/#\(1 2 3 5 7 9\)/.test(out), 'sorted: ' + out);
  });
  await check('the environment, through setenv and getenv', async () => {
    await evalTo("OSEnvironment current at: 'WASM_FFI_PAGE' put: 'yes'. OSEnvironment current at: 'WASM_FFI_PAGE'", /'yes'/);
  });
  await check('a declaration that does not match its function fails, and the VM goes on', async () => {
    await evalTo("WasmFFIPage compile: 'badStrlen: s ^ self ffiCall: #(double strlen(String s)) library: LibC'", /#badStrlen:/);
    await evalTo("[WasmFFIPage new badStrlen: 'abc'] on: PrimitiveFailed do: [:e | #failed]", /#failed/);
    await evalTo('3 + 4', /7/);
    const said = workerLog.filter(l => /FFI callout failed, its declaration does not match/.test(l));
    assert(said.length === 1, 'the worker says why, once: ' + workerLog.join(' | '));
  });
});
