// git.spec.mjs - Iceberg clones and fetches over HTTP in a browser, through a CORS proxy
//
// usage: node git.spec.mjs WEB_DIR   (see tests/wasm/lib/pw.mjs)
//
// For a build with libgit2 and its smart-HTTP transport (manifest.json says
// "git": true and "gitHttp": true: WASM_LIBGIT2=ON, with WASM_LIBGIT2_HTTP
// on, its default; make wasm-check-browser skips this spec otherwise), and
// a git with `git http-backend' (skipped without one).  tests/wasm/lib/
// git-http.mjs serves a bare repository of its own to each browser with
// `git http-backend', which answers no CORS headers, as a git server of
// another origin than the page's, and a CORS proxy in front of it, both on
// 127.0.0.1.  In the Console page:
//
//   - Settings is there, since the VM has the transport (ready.gitHttp);
//   - without a proxy, a clone of the git server fails with the
//     transport's 'request failed (CORS or network)': the browser made the
//     request but did not let the VM read the answer; and the VM goes on;
//   - the proxy typed into Settings is shown with its origin, kept in the
//     browser, and nothing went to it before;
//   - Iceberg clones the repository through it (IceRepositoryCreator, the
//     image's own LGit and the transport of the VM: each request of git's
//     smart HTTP is a synchronous XMLHttpRequest of the worker), and every
//     request that reached the git server came through the proxy;
//   - after a commit on the server, Iceberg fetches it through the proxy;
//   - after a reload, the page gives the kept proxy to the new worker,
//     which clones through it again.
//
// The browser's own complaints about the request that CORS stopped (a
// console error or a failed request, which name the git server) are what
// the second check expects: they are taken out of the errors of pw.mjs,
// whose last check is then that there were no others.  With that last
// check, 7 checks for each browser.

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { run } from './lib/pw.mjs';
import { hasGit, seedRepository, commitTo, startGitHttp } from './lib/git-http.mjs';

const webDir = path.resolve(process.argv[2] || 'build-wasm/web');
const manifest = JSON.parse(fs.readFileSync(path.join(webDir, 'manifest.json'), 'utf8'));
const skip = why => {
  console.log('skip git.spec.mjs: ' + why);
  process.exit(0);
};
if (manifest.git !== true) skip(`${webDir}/manifest.json has no libgit2 (WASM_LIBGIT2=OFF)`);
if (manifest.gitHttp !== true) skip(`${webDir}/manifest.json has no smart-HTTP transport (WASM_LIBGIT2_HTTP=OFF)`);
const gitVersion = hasGit();
if (!gitVersion) skip('no git, or no git http-backend, to serve the repositories');

const root = fs.mkdtempSync(path.join(process.env.TEST_DIR || os.tmpdir(), 'git-spec-'));
process.on('exit', () => fs.rmSync(root, { recursive: true, force: true }));
// (the layout of a repository of Iceberg: its code in src, in Tonel)
const FILES = {
  '.project': "{\n\t'srcDirectory' : 'src'\n}\n",
  'src/.properties': '{\n\t#format : #tonel\n}\n',
  'README.md': 'git.spec.mjs\n',
};
const server = await startGitHttp(root);
const gitOrigin = new URL(server.git).origin, proxyOrigin = new URL(server.proxy).origin;
console.log(`# ${gitVersion}: git server ${server.git}, CORS proxy ${server.proxy}`);

await run(async t => {
  const { page, check, assert } = t;
  const pageOrigin = new URL(t.base).origin;
  // a repository of this browser's own, and where the VM clones it
  const name = 'demo-' + t.name;
  seedRepository(root, name, FILES, 'first');
  const remote = `${server.git}${name}.git`;
  // what the proxy got from this browser
  const proxyAt = server.log.proxy.length;
  const proxied = () => server.log.proxy.slice(proxyAt);
  const term = () => page.$eval('#term', e => e.textContent);
  let markAt = 0;
  const waitStatus = (st, timeout = 30000) =>
    page.waitForFunction(s => document.getElementById('status').dataset.state === s, st, { timeout });
  async function evalTo(text, re, timeout = 60000) {
    markAt = (await term()).length;
    await page.fill('#line', text);
    await page.press('#line', 'Enter');
    await page.waitForFunction(([src, at]) => new RegExp(src).test(document.getElementById('term').textContent.slice(at)),
                               [re.source, markAt], { timeout });
    await waitStatus('waiting', timeout);
    return (await term()).slice(markAt);
  }
  // a clone with Iceberg into DIR, kept as the global GitSpecRepo: its
  // branch, or the description of the error
  const clone = dir =>
    `[ Smalltalk at: #GitSpecRepo put: (IceRepositoryCreator new remote: (IceGitRemote url: '${remote}'); ` +
    `location: FileLocator imageDirectory / 'git-spec' / '${dir}'; createRepository). ` +
    `(Smalltalk at: #GitSpecRepo) branchName ] on: Error do: [ :e | 'error ' , e class name , ': ' , e messageText ]`;
  const LOG = "((Smalltalk at: #GitSpecRepo) branch commits collect: [ :c | c comment trimBoth ]) asArray";
  // What the browser says of the request that CORS stopped (a console error,
  // a failed request), which names the git server, is expected: taken out
  // of the errors that pw.mjs checks, when the check of it is made and at
  // the end (Firefox says it later).  Every other request for the git
  // server came through the proxy, whose URLs do not name it so.
  const expected = () => {
    const said = t.errors.filter(e => e.includes(gitOrigin + '/'));
    for (const e of said) t.errors.splice(t.errors.indexOf(e), 1);
    return said;
  };
  const workerLog = [];
  page.on('worker', w => w.on('console', m => workerLog.push(m.text())));

  await page.goto(t.base + 'index.html');
  await waitStatus('waiting', 60000);

  await check('the Console offers Settings, for the smart-HTTP transport of libgit2', async () => {
    await page.waitForSelector('#settings:not([hidden])', { timeout: 5000 });
  });

  await check('without a proxy, a clone of a git server of another origin fails for CORS, and the VM goes on', async () => {
    const before = server.log.git.length;
    const out = await evalTo(clone('direct'), /'master'|'error [^']*'/);
    assert(/request failed \(CORS or network\)/.test(out), 'the clone: ' + out.trim().slice(-300));
    await evalTo('3 + 4', /7/);
    // the browser asked the git server itself, as a page of another origin
    const asked = server.log.git.slice(before);
    assert(asked.some(r => r.origin === pageOrigin && !r.proxied && /\/info\/refs\?service=git-upload-pack$/.test(r.url)),
           'the git server got the request of the page: ' + JSON.stringify(asked));
    assert(proxied().length === 0, 'nothing went to the proxy: ' + JSON.stringify(proxied()));
    const said = expected();
    console.log(`#   ${t.name} said ${said.length} things of the request that CORS stopped so far` +
                (said.length ? ': ' + said[0].slice(0, 160) : ''));
  });

  await check('the proxy typed into Settings is shown with its origin and kept in the browser', async () => {
    await page.click('#settings');
    await page.waitForSelector('#settings-dialog[open]', { timeout: 5000 });
    await page.fill('#git-proxy', server.proxy);
    const state = await page.$eval('#git-proxy-state', e => e.textContent);
    assert(state.includes(proxyOrigin), 'the dialog names the origin of the proxy: ' + state);
    markAt = (await term()).length;
    await page.click('#settings-save');
    await page.waitForFunction(at => /through the proxy at /.test(document.getElementById('term').textContent.slice(at)),
                               markAt, { timeout: 5000 });
    assert((await term()).slice(markAt).includes(proxyOrigin), 'the note names the origin of the proxy');
    const kept = await page.evaluate(() => localStorage.getItem('pharo-wasm.gitProxy'));
    assert(kept === server.proxy, 'kept: ' + JSON.stringify(kept));
    assert(proxied().length === 0, 'nothing went to the proxy yet: ' + JSON.stringify(proxied()));
  });

  await check('Iceberg clones through the proxy, which alone asks the git server', async () => {
    const before = server.log.git.length, t0 = Date.now();
    const out = await evalTo(clone('proxied'), /'master'|'error [^']*'/);
    const ms = Date.now() - t0;
    assert(/'master'/.test(out), 'the clone: ' + out.trim().slice(-300));
    const log = await evalTo(LOG, /#\([^)]*\)/);
    assert(/#\('first'\)/.test(log), 'the log of the clone: ' + log.trim().slice(-200));
    const asked = server.log.git.slice(before);
    assert(asked.length > 0 && asked.every(r => r.proxied), 'the git server got only what the proxy forwarded: ' +
           JSON.stringify(asked));
    assert(proxied().length > 0 && proxied().every(r => r.origin === pageOrigin),
           'the proxy got the requests of the page only: ' + JSON.stringify(proxied()));
    console.log(`#   ${t.name}: cloned in ${ms} ms, ${asked.length} requests through the proxy`);
  });

  await check('after a commit on the server, Iceberg fetches it through the proxy', async () => {
    commitTo(root, name, { 'second.txt': 'second\n' }, 'second');
    const before = server.log.git.length;
    const out = await evalTo("[ (Smalltalk at: #GitSpecRepo) fetch. ((Smalltalk at: #GitSpecRepo) remoteBranchNamed: " +
                             "'origin/master') commit comment trimBoth ] on: Error do: [ :e | 'error ' , e messageText ]",
                             /'second'|'error [^']*'|'first'/);
    assert(/'second'/.test(out), 'the remote branch after the fetch: ' + out.trim().slice(-300));
    const asked = server.log.git.slice(before);
    assert(asked.some(r => r.method === 'POST' && /git-upload-pack$/.test(r.url)) && asked.every(r => r.proxied),
           'a fetch, through the proxy: ' + JSON.stringify(asked));
  });

  await check('after a reload, the kept proxy goes to the new worker, which clones through it', async () => {
    await page.reload();
    await waitStatus('waiting', 60000);
    const text = await term();
    assert(text.includes('Git requests go through the proxy at ' + proxyOrigin), 'the note at the start: ' + text.slice(0, 400));
    const before = server.log.git.length;
    const out = await evalTo(clone('reloaded'), /'master'|'error [^']*'/);
    assert(/'master'/.test(out), 'the clone: ' + out.trim().slice(-300));
    const log = await evalTo(LOG, /#\([^)]*\)/);
    assert(/#\('second' 'first'\)/.test(log), 'the log of the clone: ' + log.trim().slice(-200));
    const asked = server.log.git.slice(before);
    assert(asked.length > 0 && asked.every(r => r.proxied), 'through the proxy: ' + JSON.stringify(asked));
    const said = workerLog.filter(l => /FFI callout (failed|trapped)/.test(l));
    assert(said.length === 0, 'no failed callout: ' + said.join(' | '));
  });
  expected();
});
