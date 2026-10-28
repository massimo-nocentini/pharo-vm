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
// 127.0.0.1; the git server redirects a remote named without its .git, as
// GitLab and Codeberg do (its option redirect).
//
// No request may go to the public proxy that a site has by default
// (cors.isomorphic-git.org, WASM_GIT_PROXY): pw.mjs aborts any and counts
// it as an error.  The site's default that the pages see is the one of
// t.manifestOverride, which this spec makes the local proxy (pw.mjs makes
// it a dead one otherwise).  In the Console page:
//
//   - Settings is there, since the VM has the transport (ready.gitHttp);
//   - with no choice of the user (pharo-wasm.gitProxy absent), the page
//     gives the worker null (init.gitProxy), which then sends git's
//     requests to the site's default (ready.gitProxy): Iceberg clones
//     through it, and the page says once that the requests go through the
//     site's public proxy (the one-time note, pharo-wasm.gitProxyNoted);
//   - the first commit of a user that git does not know yet (the HOME of
//     the VM has no .gitconfig at first) goes through Iceberg's dialog of
//     the missing properties, as the world page shows it: here the dialog
//     (IceTipGitUsernameAndEmailDialogPresenter) is made as Iceberg makes
//     it, not opened, filled in with 'Save properties as global?' checked,
//     and accepted, which resumes the commit; then a full garbage
//     collection, whose finalization frees the LGitConfig that the dialog
//     made, with git_config_free and no error in the terminal, and the next
//     commit must work (on Pharo 13 and later only the free function of
//     Pharo 12, which packaging/emscripten/st/web-libgit.st compiles, makes
//     that finalization harmless), without a failed callout;
//   - that user, written into HOME/.gitconfig, is kept with the image:
//     after Save and a reload HOME is /pharo/home, its .gitconfig is there,
//     and a commit no longer asks for the user;
//   - the clone is kept with the image (the files of the image, saved with
//     it in the browser): after Save and a reload, its directory and its
//     .git are there, Iceberg fetches it again, and the page does not say
//     the one-time note again;
//   - with None chosen in Settings, a clone of the git server fails with
//     the transport's 'request failed (CORS or network)', which points to
//     Settings: the browser made one request (Iceberg does not try again
//     when there is no proxy: a CORS refusal stays one), the page says why
//     in a notice whose button opens Settings, and the VM goes on;
//   - the proxy typed into Settings is shown with its origin, kept in the
//     browser, and nothing went to it since None;
//   - Iceberg clones the repository through it (IceRepositoryCreator, the
//     image's own LGit and the transport of the VM: each request of git's
//     smart HTTP is a synchronous XMLHttpRequest of the worker), and every
//     request that reached the git server came through the proxy;
//   - a remote whose URL has a user name and a token clones through it
//     too, and the proxy never sees them;
//   - after a commit on the server, Iceberg fetches it through the proxy;
//   - a remote named without its .git, which the git server redirects,
//     clones through the proxy: the browser follows the redirect through
//     the proxy, and the transport sends the next requests to the new URL
//     (a POST to the old one would get a 422);
//   - after a reload, the page gives the kept proxy to the new worker,
//     which clones through it again.
//
// Then on the world page (world.html), which runs the image saved above:
// its Settings show the kept proxy, and None and the site's default go to
// its VM (the "gitProxy" message) and into the browser.
//
// Last, back in the Console, the stock image of the build (<web>/../image/
// stock), which nothing prepared, opened from disk: its boot, which
// evaluates st/web-libgit.st (web-repl.st), gives LGitConfig the free
// function git_config_free when it lacks it (Pharo 13 and later), in a
// protocol of LibGit-Core, which neither makes a package OSWindow-Web nor
// leaves LibGit-Core modified for Monticello; and the first commit through
// the dialog, a full garbage collection and the next commit work.  Then the
// notebook kernel boots that image as it was kept (web-notebook.st, which
// evaluates web-libgit.st too), and the same flow in a cell works.  Without
// that evaluation at the boot, both fail on a stock Pharo 13 or later (the
// free function is object_free:, and the commit after the garbage
// collection fails).
//
// The browser's own complaints about the request that CORS stopped (a
// console error or a failed request, which name the git server) are what
// the check of None expects: they are taken out of the errors of pw.mjs,
// whose last check is then that there were no others.  With that last
// check, 16 checks for each browser.

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
const server = await startGitHttp(root, { redirect: true });
const gitOrigin = new URL(server.git).origin, proxyOrigin = new URL(server.proxy).origin;
console.log(`# ${gitVersion}: git server ${server.git}, CORS proxy ${server.proxy}`);

const NOTE_ONCE = 'Git requests (Iceberg clones, Metacello github://) go through the public CORS proxy ' + proxyOrigin +
  ', which sees what you clone.';

await run(async t => {
  const { page, check, assert } = t;
  const pageOrigin = new URL(t.base).origin;
  // the site's default proxy, for every page of this browser: the local one
  t.manifestOverride.gitProxyDefault = server.proxy;
  // a repository of this browser's own, and where the VM clones it
  const name = 'demo-' + t.name;
  seedRepository(root, name, FILES, 'first');
  const remote = `${server.git}${name}.git`;
  // what the proxy and the git server got from this browser since a mark
  let proxyAt = server.log.proxy.length;
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
  // a clone with Iceberg of URL (by default the remote) into DIR, kept as
  // the global GitSpecRepo: its branch, or the description of the error
  const clone = (dir, url = remote) =>
    `[ Smalltalk at: #GitSpecRepo put: (IceRepositoryCreator new remote: (IceGitRemote url: '${url}'); ` +
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
  // (a reload or a goto while the VM ran may ask first: it may go)
  page.on('dialog', d => d.accept());
  // What the page gave its workers of the proxy ([type, gitProxy] of each
  // "init" and "gitProxy"), what they said of it (ready.gitProxy and
  // ready.gitProxyDefault) and of git's requests (their "git"), and the
  // texts of the notice: for each document of the page
  await page.addInitScript(() => {
    const post = Worker.prototype.postMessage;
    window.__git = { sent: [], ready: [], events: [], notices: [] };
    Worker.prototype.postMessage = function (m, transfer) {
      if (!this.__heard) {
        this.__heard = true;
        this.addEventListener('message', e => {
          const d = e.data || {};
          if (d.type === 'ready') window.__git.ready.push({ gitProxy: d.gitProxy, gitProxyDefault: d.gitProxyDefault });
          else if (d.type === 'git') window.__git.events.push(d);
        });
      }
      if (m && (m.type === 'init' || m.type === 'gitProxy')) window.__git.sent.push([m.type, m.gitProxy]);
      return post.call(this, m, transfer);
    };
    document.addEventListener('DOMContentLoaded', () => {
      const text = document.getElementById('notice-text');
      if (text) new MutationObserver(() => window.__git.notices.push(text.textContent))
        .observe(text, { childList: true, characterData: true, subtree: true });
    });
  });
  const spied = () => page.evaluate(() => window.__git);
  const notice = () => page.evaluate(() => document.getElementById('notice').hidden ? null
    : { text: document.getElementById('notice-text').textContent,
        action: document.getElementById('notice-action').hidden ? '' : document.getElementById('notice-action').textContent });
  const stored = () => page.evaluate(() => localStorage.getItem('pharo-wasm.gitProxy'));
  const ready = async () => {
    await page.waitForFunction(() => window.__git.ready.length > 0, null, { timeout: 60000 });
    return (await spied()).ready.slice(-1)[0];
  };

  await page.goto(t.base + 'index.html');
  await waitStatus('waiting', 60000);

  await check('the Console offers Settings, for the smart-HTTP transport of libgit2', async () => {
    await page.waitForSelector('#settings:not([hidden])', { timeout: 5000 });
  });

  await check('by default the VM uses the site\'s proxy: Iceberg clones through it, and the page says so once', async () => {
    const spy = await spied();
    assert(await stored() === null, 'no choice of the user: ' + await stored());
    assert(JSON.stringify(spy.sent) === '[["init",null]]', 'the page gives the worker null: ' + JSON.stringify(spy.sent));
    const r = await ready();
    assert(r.gitProxy === server.proxy && r.gitProxyDefault === server.proxy, 'the worker uses the default: ' + JSON.stringify(r));
    const text = await term();
    assert(text.includes('Git requests go through the proxy at ' + proxyOrigin + " (this site's default) (Settings)."),
           'the note at the start: ' + text.slice(0, 600));
    proxyAt = server.log.proxy.length;
    const before = server.log.git.length;
    const out = await evalTo(clone('default'), /'master'|'error [^']*'/);
    assert(/'master'/.test(out), 'the clone: ' + out.trim().slice(-300));
    const asked = server.log.git.slice(before);
    assert(asked.length > 0 && asked.every(r => r.proxied), 'through the proxy: ' + JSON.stringify(asked));
    assert(proxied().length > 0 && proxied().every(r => r.origin === pageOrigin), 'the proxy got the page\'s requests');
    const events = (await spied()).events;
    assert(events.length && events[0].event === 'request' && events[0].proxy === proxyOrigin,
           'the worker says that git asked, through the proxy: ' + JSON.stringify(events));
    const said = (await term()).split(NOTE_ONCE).length - 1;
    assert(said === 1, `the one-time note, once in the Console (${said})`);
    assert((await spied()).notices.includes(NOTE_ONCE), 'and in a notice: ' + JSON.stringify((await spied()).notices));
    const noted = await page.evaluate(() => localStorage.getItem('pharo-wasm.gitProxyNoted'));
    assert(noted === proxyOrigin, 'noted: ' + noted);
  });

  // A commit of the repository GitSpecIdRepo, which answers its short id
  // and how many times a commit asked for the user so far (GitSpecDialogs):
  // when it asks, the dialog answers, made as
  // IceTipInteractiveErrorVisitor>>#visitMissingCredentialsError: makes it,
  // and its Ok (IceTipGitUsernameAndEmailDialogPresenter>>#accept) resumes
  // the commit
  const commitAs = message =>
    `| r d | r := Smalltalk at: #GitSpecIdRepo. [ 'commit ' , ([ (r commitWithMessage: '${message}') shortId ] ` +
    `on: IceGitUsernameOrEmailNotFound do: [ :missing | ` +
    `Smalltalk at: #GitSpecDialogs put: (Smalltalk at: #GitSpecDialogs) + 1. ` +
    `d := IceTipGitUsernameAndEmailDialogPresenter newApplication: SpApplication new model: r. d asModalWindow. ` +
    `d nameInput text: 'Git Spec User'. d emailInput text: 'spec@example.org'. d globalCheckbox state: true. ` +
    `d onAccept: [ missing resume ]. d accept ]) , ' dialogs ' , (Smalltalk at: #GitSpecDialogs) printString ] ` +
    `on: Error do: [ :e | 'error ' , e class name , ': ' , e messageText ]`;
  const COMMITTED = /'commit [0-9a-f]{7} dialogs \d+'|'error [^']*'/;
  const ID_LOG = "((Smalltalk at: #GitSpecIdRepo) branch commits collect: [ :c | c comment , ' (' , c author , ')' ]) asArray";
  const trapped = () => workerLog.filter(l => /FFI callout (failed|trapped)/.test(l));
  // The C functions that the method of LGitConfig's free function names in
  // its callout ('missing' without that method): git_config_free once the
  // image is set up
  const FREE_CALLS = "((LGitConfig class lookupSelector: LGitConfig freeFunctionSelector) " +
    "ifNil: [ 'missing' ] ifNotNil: [ :m | ' ' join: ((m ast allChildren select: [ :n | n isLiteralNode and: [ n value isArray ] ]) " +
    "flatCollect: [ :n | (n value select: [ :e | e isSymbol and: [ e beginsWith: 'git_' ] ]) collect: [ :e | e asString ] ]) ])";

  await check('a first commit asks for the user, the dialog sets it globally, and after a full GC the next commit works', async () => {
    const from = (await term()).length;
    const start = await evalTo("| r c | Smalltalk at: #GitSpecDialogs put: 0. " +
                               "r := IceRepositoryCreator new location: FileLocator imageDirectory / 'git-spec' / 'identity'; " +
                               "subdirectory: 'src'; createNewRepositoryNamed: 'identity'. r register. " +
                               "Smalltalk at: #GitSpecIdRepo put: r. " +
                               "c := (Object << #GitSpecIdentityDemo) package: 'GitSpecIdentityDemo'; install. " +
                               "c compile: 'one ^ 1'. r workingCopy addPackageNamed: 'GitSpecIdentityDemo'. " +
                               "'ready, .gitconfig ' , (FileLocator home / '.gitconfig') exists printString",
                               /'ready[^']*'|Error|error/);
    assert(/'ready, \.gitconfig false'/.test(start), 'a repository, a package, and no user yet: ' + start.trim().slice(-300));
    const first = await evalTo(commitAs('first'), COMMITTED);
    assert(/'commit [0-9a-f]{7} dialogs 1'/.test(first), 'the first commit, through the dialog: ' + first.trim().slice(-300));
    const gc = await evalTo("Smalltalk garbageCollect. (Delay forMilliseconds: 300) wait. Smalltalk garbageCollect. " +
                            "(Delay forMilliseconds: 300) wait. 'configurations ' , LGitConfig allInstances size printString , " +
                            "', freed by ' , " + FREE_CALLS,
                            /'configurations [^']*'/);
    assert(/'configurations 0, freed by git_config_free'/.test(gc),
           'the LGitConfig objects of the dialog are freed by git_config_free: ' + (gc.match(/'configurations [^']*'/g) || [gc.trim().slice(-200)]).pop());
    await evalTo("(Smalltalk at: #GitSpecIdentityDemo) compile: 'two ^ 2'. #compiled", /#compiled/);
    const second = await evalTo(commitAs('second'), COMMITTED);
    assert(/'commit [0-9a-f]{7} dialogs 1'/.test(second), 'the commit after the GC, without the dialog: ' + second.trim().slice(-300));
    const log = await evalTo(ID_LOG, /#\([^)]*\)/);
    assert(log.includes("#('second (Git Spec User)' 'first (Git Spec User)')"), 'the log: ' + log.trim().slice(-300));
    assert(trapped().length === 0, 'no failed callout: ' + trapped().join(' | '));
    // (an error of the finalization process, a free function that is not
    // there say, only shows in the terminal)
    const errors = (await term()).slice(from).match(/\bError: [A-Za-z][^\n]*/g);
    assert(!errors, 'no error in the terminal: ' + (errors || []).join(' | ').slice(0, 400));
  });

  await check('that user is kept with the saved image: after a reload HOME/.gitconfig is there, and a commit asks no more', async () => {
    await page.click('#save');
    await page.waitForFunction(() => !document.getElementById('notice').hidden &&
                               /^The image is saved in this browser/.test(document.getElementById('notice-text').textContent),
                               null, { timeout: 120000 });
    await waitStatus('waiting', 60000);
    await page.reload();
    await waitStatus('waiting', 60000);
    assert(/[?&]slot=/.test(page.url()), 'the saved image runs: ' + page.url());
    const home = await evalTo("'home ' , (OSEnvironment current at: 'HOME' ifAbsent: [ 'none' ]) , ' ' , " +
                              "((FileLocator home / '.gitconfig') exists ifTrue: [ (FileLocator home / '.gitconfig') contents " +
                              "lines detect: [ :l | l includesSubstring: 'name =' ] ifNone: [ 'no name' ] ] ifFalse: [ 'no .gitconfig' ]) trimBoth",
                              /'home [^']*'/);
    assert(/'home \/pharo\/home name = Git Spec User'/.test(home), 'HOME and its .gitconfig: ' + home.trim().slice(-300));
    await evalTo("Smalltalk at: #GitSpecDialogs put: 0. (Smalltalk at: #GitSpecIdentityDemo) compile: 'three ^ 3'. #compiled", /#compiled/);
    const third = await evalTo(commitAs('third'), COMMITTED);
    assert(/'commit [0-9a-f]{7} dialogs 0'/.test(third), 'a commit that does not ask for the user: ' + third.trim().slice(-300));
    const log = await evalTo(ID_LOG, /#\([^)]*\)/);
    assert(log.includes("#('third (Git Spec User)' 'second (Git Spec User)' 'first (Git Spec User)')"),
           'the log: ' + log.trim().slice(-300));
    assert(trapped().length === 0, 'no failed callout: ' + trapped().join(' | '));
  });

  await check('the clone is saved with the image: after a reload its .git is there, Iceberg fetches it, and no note again', async () => {
    markAt = (await term()).length;
    await page.click('#save');
    await page.waitForFunction(() => !document.getElementById('notice').hidden &&
                               /^The image is saved in this browser/.test(document.getElementById('notice-text').textContent),
                               null, { timeout: 120000 });
    await waitStatus('waiting', 60000);
    await page.reload();
    await waitStatus('waiting', 60000);
    assert(/[?&]slot=/.test(page.url()), 'the saved image runs: ' + page.url());
    const there = await evalTo("(FileLocator imageDirectory / 'git-spec' / 'default' / '.git' / 'HEAD') exists printString", /'(true|false)'/);
    assert(/'true'/.test(there), 'the .git of the clone: ' + there.trim().slice(-200));
    const before = server.log.git.length;
    const out = await evalTo("[ (Smalltalk at: #GitSpecRepo) fetch. ((Smalltalk at: #GitSpecRepo) remoteBranchNamed: " +
                             "'origin/master') commit comment trimBoth ] on: Error do: [ :e | 'error ' , e messageText ]",
                             /'first'|'error [^']*'/);
    assert(/'first'/.test(out), 'the fetch of the clone kept: ' + out.trim().slice(-300));
    const asked = server.log.git.slice(before);
    assert(asked.length > 0 && asked.every(r => r.proxied), 'a fetch, through the proxy: ' + JSON.stringify(asked));
    const spy = await spied();
    assert(spy.events.some(e => e.event === 'request' && e.proxy === proxyOrigin), 'git asked again: ' + JSON.stringify(spy.events));
    assert(!(await term()).includes(NOTE_ONCE) && !spy.notices.includes(NOTE_ONCE), 'the one-time note, not again');
  });

  await check('with None, a clone of a git server of another origin fails for CORS once, points to Settings, and the VM goes on', async () => {
    await page.click('#settings');
    await page.waitForSelector('#settings-dialog[open]', { timeout: 5000 });
    await page.click('#git-proxy-mode-none');
    await page.click('#settings-save');
    assert(await stored() === 'none', 'None kept: ' + await stored());
    assert(JSON.stringify((await spied()).sent.slice(-1)) === '[["gitProxy",""]]', 'and given to the worker: ' +
           JSON.stringify((await spied()).sent));
    proxyAt = server.log.proxy.length;
    const before = server.log.git.length;
    const out = await evalTo(clone('direct'), /'master'|'error [^']*'/);
    assert(/request failed \(CORS or network\)/.test(out) && /Settings/.test(out), 'the clone: ' + out.trim().slice(-300));
    const shown = await notice();
    assert(shown && shown.text === "The browser blocked git's request to " + new URL(server.git).host +
           ' (CORS): git over HTTPS needs a proxy here.' && shown.action === 'Settings', 'the notice: ' + JSON.stringify(shown));
    await evalTo('3 + 4', /7/);
    // the browser asked the git server itself, as a page of another origin,
    // once: no retry without a proxy
    const asked = server.log.git.slice(before);
    const refs = asked.filter(r => /\/info\/refs\?service=git-upload-pack$/.test(r.url));
    assert(refs.length === 1 && refs[0].origin === pageOrigin && !refs[0].proxied,
           'one request of the page reached the git server: ' + JSON.stringify(asked));
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
    assert(await stored() === server.proxy, 'kept: ' + JSON.stringify(await stored()));
    assert(JSON.stringify((await spied()).sent.slice(-1)) === JSON.stringify([['gitProxy', server.proxy]]),
           'given to the worker: ' + JSON.stringify((await spied()).sent));
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

  await check('a remote with a user name and a token in its URL clones through the proxy, which never sees them', async () => {
    const withUser = server.git.replace(/^(https?:\/\/)/, '$1git-spec-user:s3cret-token@') + name + '.git';
    proxyAt = server.log.proxy.length;
    const before = server.log.git.length;
    const out = await evalTo(clone('with-user', withUser), /'master'|'error [^']*'/);
    assert(/'master'/.test(out), 'the clone: ' + out.trim().slice(-300));
    assert(proxied().length > 0 && proxied().every(r => !/git-spec-user|s3cret-token/.test(r.url)),
           'the proxy got no user name or token: ' + JSON.stringify(proxied().map(r => r.url)));
    assert(server.log.git.slice(before).every(r => r.proxied && !/s3cret-token/.test(r.url)), 'nor the git server');
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

  await check('a remote without its .git, which the server redirects, clones through the proxy', async () => {
    const before = server.log.git.length;
    const out = await evalTo(clone('redirected', server.git + name), /'master'|'error [^']*'/);
    assert(/'master'/.test(out), 'the clone: ' + out.trim().slice(-300));
    const log = await evalTo(LOG, /#\([^)]*\)/);
    assert(/#\('second' 'first'\)/.test(log), 'the log of the clone: ' + log.trim().slice(-200));
    const asked = server.log.git.slice(before);
    const said = asked.map(r => `${r.method} ${r.url.split('?')[0]} ${r.status}`);
    assert(asked.every(r => r.proxied), 'through the proxy: ' + JSON.stringify(asked));
    assert(said[0] === `GET /${name}/info/refs 301` && said.includes(`GET /${name}.git/info/refs 200`) &&
           said.includes(`POST /${name}.git/git-upload-pack 200`) && !said.some(s => s.startsWith(`POST /${name}/`)),
           'the redirect, then the new URL: ' + said.join(', '));
  });

  await check('after a reload, the kept proxy goes to the new worker, which clones through it', async () => {
    await page.reload();
    await waitStatus('waiting', 60000);
    const text = await term();
    assert(text.includes('Git requests go through the proxy at ' + proxyOrigin + ' (Settings).'),
           'the note at the start, not of the default: ' + text.slice(0, 600));
    assert(JSON.stringify((await spied()).sent) === JSON.stringify([['init', server.proxy]]),
           'init: ' + JSON.stringify((await spied()).sent));
    assert((await ready()).gitProxy === server.proxy, 'ready: ' + JSON.stringify(await ready()));
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

  await check('Settings on the world page show the kept proxy; None and the default go to its VM', async () => {
    await page.goto(t.base + 'world.html');
    await page.waitForFunction(() => window.PharoWorld && window.PharoWorld.stats.frames > 0, null, { timeout: 90000 });
    assert(await page.isVisible('#settings'), 'Settings shown');
    assert(JSON.stringify((await spied()).sent) === JSON.stringify([['init', server.proxy]]),
           'init: ' + JSON.stringify((await spied()).sent));
    const mode = () => page.evaluate(() => ['default', 'none', 'custom'].find(m => document.getElementById('git-proxy-mode-' + m).checked));
    await page.click('#settings');
    await page.waitForSelector('#settings-dialog[open]', { timeout: 5000 });
    assert(await mode() === 'custom' && await page.inputValue('#git-proxy') === server.proxy,
           'the kept proxy: ' + await mode() + ' ' + await page.inputValue('#git-proxy'));
    assert(await page.textContent('#git-proxy-default-url') === proxyOrigin, 'the default named');
    await page.click('#git-proxy-mode-none');
    await page.click('#settings-save');
    assert(await stored() === 'none' && JSON.stringify((await spied()).sent.slice(-1)) === '[["gitProxy",""]]',
           'None: ' + JSON.stringify((await spied()).sent));
    await page.click('#settings');
    await page.click('#git-proxy-mode-default');
    await page.click('#settings-save');
    assert(await stored() === null && JSON.stringify((await spied()).sent.slice(-1)) === '[["gitProxy",null]]',
           'the default: ' + JSON.stringify((await spied()).sent));
  });

  // The flow of a commit through the dialog, a full garbage collection and
  // another commit, in one evaluation, in a repository of its own in DIR
  // (under /pharo/git-spec) of a package of its own, NAME: what it answers
  // says LGitConfig's free function, where its method is, whether there is
  // a package OSWindow-Web and whether Monticello sees LibGit-Core
  // modified, then the commits, the dialogs, the configurations left after
  // the collection and the log, or the error of the step that failed; and
  // first the major version of Pharo, and whether the class OTHER is there
  const flowIn = (dir, name, other) =>
    `| r c d dialogs commit free where | dialogs := 0. ` +
    `free := LGitConfig class lookupSelector: LGitConfig freeFunctionSelector. ` +
    `where := 'stock ' , SystemVersion current major printString , ' ' , (Smalltalk hasClassNamed: #${other}) printString , ` +
    `'; free ' , LGitConfig freeFunctionSelector , ' ' , ${FREE_CALLS} , ' in ' , free package name , ` +
    `', OSWindow-Web ' , (PackageOrganizer default hasPackage: 'OSWindow-Web') printString , ` +
    `', LibGit-Core modified ' , (MCWorkingCopy forPackageNamed: 'LibGit-Core') modified printString. ` +
    `commit := [ :m | ([ r commitWithMessage: m ] on: IceGitUsernameOrEmailNotFound do: [ :missing | ` +
    `dialogs := dialogs + 1. d := IceTipGitUsernameAndEmailDialogPresenter newApplication: SpApplication new model: r. ` +
    `d asModalWindow. d nameInput text: 'Git Spec Stock'. d emailInput text: 'stock@example.org'. ` +
    `d globalCheckbox state: true. d onAccept: [ missing resume ]. d accept ]) shortId ]. ` +
    `[ r := IceRepositoryCreator new location: FileLocator imageDirectory / 'git-spec' / '${dir}'; ` +
    `subdirectory: 'src'; createNewRepositoryNamed: '${dir}'. r register. ` +
    `c := (Object << #${name}) package: '${name}'; install. c compile: 'one ^ 1'. ` +
    `r workingCopy addPackageNamed: '${name}'. commit value: 'first'. ` +
    `Smalltalk garbageCollect. (Delay forMilliseconds: 300) wait. Smalltalk garbageCollect. (Delay forMilliseconds: 300) wait. ` +
    `c compile: 'two ^ 2'. commit value: 'second'. ` +
    `where , '; dialogs ' , dialogs printString , ', configurations ' , LGitConfig allInstances size printString , ` +
    `', log ' , (' / ' join: (r branch commits collect: [ :e | e comment , ' (' , e author , ')' ])) ] ` +
    `on: Error do: [ :e | where , '; error ' , e class name , ': ' , e messageText ]`;
  // what that flow must answer, after its 'stock <major> <other>; '
  const FLOWED = /^'stock \d+ false; free config_free: git_config_free in LibGit-Core, OSWindow-Web false, LibGit-Core modified false; dialogs 1, configurations 0, log second \(Git Spec Stock\) \/ first \(Git Spec Stock\)/;
  const stock = path.join(webDir, '..', 'image', 'stock');
  const stockImage = fs.existsSync(stock) && fs.readdirSync(stock).find(f => f.endsWith('.image'));

  if (stockImage) await check('a stock image opened in the Console gets the free function at its boot: the dialog, a full GC and the next commit work', async () => {
    await page.goto(t.base + 'index.html');
    await waitStatus('waiting', 60000);
    const from = (await term()).length;
    markAt = from;
    await page.setInputFiles('#open-file', [path.join(stock, stockImage), path.join(stock, stockImage.replace(/\.image$/, '.changes'))]);
    await page.waitForFunction(at => /Started the opened image[\s\S]*st> $/.test(document.getElementById('term').textContent.slice(at)),
                               from, { timeout: 120000 });
    await waitStatus('waiting', 60000);
    // (OSWebDriver: the stock image has no OSWindow-Web)
    const out = await evalTo(flowIn('stock-console', 'GitSpecStockConsole', 'OSWebDriver'), /\n'stock [^']*'\n/, 120000);
    const said = (out.match(/\n('stock [^']*')\n/) || [, out.trim().slice(-400)])[1];
    console.log(`#   ${t.name}, in the Console: ${said}`);
    assert(FLOWED.test(said), 'the flow: ' + said);
    assert(trapped().length === 0, 'no failed callout: ' + trapped().join(' | '));
    const errors = (await term()).slice(from).match(/\bError: [A-Za-z][^\n]*/g);
    assert(!errors, 'no error in the terminal: ' + (errors || []).join(' | ').slice(0, 400));
    // (kept in a slot, which the notebook boots)
    await page.waitForFunction(() => /The opened image is now kept in this browser|cannot open the Pharo world/.test(
      document.getElementById('notice-text').textContent), null, { timeout: 60000 });
  });

  if (stockImage) await check('the notebook kernel boots that stock image, which its boot gives the free function too: the same flow in a cell works', async () => {
    await page.click('#tab-notebook');
    await page.waitForFunction(() => /^ready$/.test(document.getElementById('nb-status').dataset.state), null, { timeout: 120000 });
    const before = await page.$$eval('#nb-cells > li', l => l.length);
    await page.click('#nb-end-code');
    const i = await page.$$eval('#nb-cells > li', l => l.length) - 1;
    assert(i === before, 'a new cell');
    const src = page.locator('#nb-cells > li').nth(i).locator('.nb-src');
    // (GitSpecStockConsole: the Console defined it, but did not save)
    await src.fill(flowIn('stock-notebook', 'GitSpecStockNotebook', 'GitSpecStockConsole'));
    await src.press('Control+Enter');
    await page.waitForFunction(i => /^(ok|error|interrupted|cancelled)$/.test(document.querySelectorAll('#nb-cells > li')[i].dataset.status),
                               i, { timeout: 120000 });
    const cell = await page.$eval(`#nb-cells > li:nth-child(${i + 1})`, e => ({
      status: e.dataset.status, values: [...e.querySelectorAll('.nb-value')].map(v => v.textContent),
      error: (e.querySelector('.nb-error-msg') || {}).textContent || '' }));
    const said = cell.values.join(' ');
    console.log(`#   ${t.name}, the notebook kernel: ${said.slice(0, 400)}`);
    assert(cell.status === 'ok', 'the cell: ' + JSON.stringify(cell).slice(0, 400));
    // (the image the kernel booted is the stock one as it was opened: not
    // the Console's, which defined GitSpecStockConsole, whose false FLOWED
    // wants)
    assert(FLOWED.test(said), 'the flow: ' + said);
    assert(trapped().length === 0, 'no failed callout: ' + trapped().join(' | '));
  });
  expected();
});
