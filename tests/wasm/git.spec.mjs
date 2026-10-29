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
// GitLab and Codeberg do (its option redirect), and takes a push only with
// the HTTP Basic credentials PUSHER, as github.com takes one with a token
// (its options push and auth); its proxy forwards the Authorization of a
// request, drops the WWW-Authenticate of a response, and makes any Location
// a path of its own, as the proxy of isomorphic-git does; and its mover,
// named localhost, stands for a host that redirects to the git server (its
// option mover).
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
//     too, and the proxy never sees them, in a URL nor in an Authorization
//     (the git server does not ask for them);
//   - after a commit on the server, Iceberg fetches it through the proxy;
//   - a remote named without its .git, which the git server redirects,
//     clones through the proxy: the browser follows the redirect through
//     the proxy, and the transport sends the next requests to the new URL
//     (a POST to the old one would get a 422);
//   - after a reload, the page gives the kept proxy to the new worker,
//     which clones through it again;
//   - with the site's default proxy chosen again, a push of a new commit
//     fails at its first 401, before Iceberg is asked for credentials
//     (Module.gitHttpCredentials false: the transport sends none through a
//     public proxy that the user did not choose): one request, without an
//     Authorization, the worker says why ('failed', credentials 'refused'),
//     and the notice offers that proxy for them;
//   - the button of the notice makes that proxy My proxy, given to the
//     worker; a push without credentials in IceCredentialStore then fails
//     with the transport's 401 of none given, and once Iceberg's store has
//     them the push lands: a 401, then the same GET and the POST of the
//     pack with an Authorization, all through the proxy, no URL with the
//     credentials, the commit in the server's git log, no 'failed' for the
//     401 that credentials answered (the push waits for the worker's
//     throttle of 'failed', GIT_FAILED_MS, to be over first), and the note
//     that credentials went through the proxy, once
//     (pharo-wasm.gitCredentialsNoted);
//   - a pull fast-forwards to a commit that the spec made on the server,
//     and loads it; after a local commit and another on the server, the
//     push is refused (IceRemoteDesynchronized), the pull merges (a commit
//     of 2 parents, the methods of both sides loaded), and the merge pushes,
//     as the server's git logs it;
//   - after a local commit and one on the server that change the same
//     method, the pull fails at once with IceMergeAborted (the Console has
//     no world: web-libgit.st makes the merge ask no dialog, which nothing
//     would answer), and the repository is as it was;
//   - with credentials stored for localhost (the mover) only, a push to a
//     remote of the mover whose advertisement it redirects (301) to the git
//     server, which asks for credentials, fails with the transport's
//     'redirected the request to <git server>', and the git server gets no
//     Authorization; and a push to one that asks for them (401) and then
//     redirects (302) the request that carries them: the proxy makes the
//     redirect a path of its own, so the browser sends them again, and the
//     git server gets them once (a synchronous request cannot refuse the
//     redirect), but the transport stops there, nothing follows, the
//     worker says so ('failed', credentials 'redirected', redirectedTo the
//     git server), and the notice says to revoke the token;
//   - in a context of the browser without the routes of pw.mjs (with them,
//     Chromium answers the CORS preflights itself), a push with stored
//     credentials through the proxy (My proxy, from localStorage) makes the
//     browser ask the proxy first whether it takes an Authorization (an
//     OPTIONS whose Access-Control-Request-Headers has it), and lands.
//
// Then on the world page, booted with tests/wasm/st/git-world.st, which
// evaluates what the spec writes in its UI process, as a click would: a
// push without stored credentials returns at once with the 401 of none
// given (OSWebCredentialsProvider gives none inside libgit2's callback,
// where Iceberg's own dialog would hang the page), the world is not busy,
// Iceberg's dialog 'Please log in to <host>' opens after it, and once its
// Ok (made by the spec) stored what it was given, the push lands.
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
// check, 23 checks for each browser.

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { run } from './lib/pw.mjs';
import { hasGit, seedRepository, commitTo, fileOf, logOf, startGitHttp } from './lib/git-http.mjs';

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
// (a push asks for these credentials, as github.com asks for a token)
const PUSHER = { user: 'git-spec-pusher', password: 's3cret-push-token' };
const server = await startGitHttp(root, { redirect: true, push: true, auth: PUSHER, mover: true });
const gitOrigin = new URL(server.git).origin, proxyOrigin = new URL(server.proxy).origin;
console.log(`# ${gitVersion}: git server ${server.git}, mover ${server.moved}, CORS proxy ${server.proxy}`);

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
    assert(proxied().length > 0 && proxied().every(r => !/git-spec-user|s3cret-token/.test(r.url) && !r.authorization),
           'the proxy got no user name or token, in a URL nor in an Authorization: ' +
           JSON.stringify(proxied().map(r => [r.url, r.authorization])));
    assert(server.log.git.slice(before).every(r => r.proxied && !/s3cret-token/.test(r.url) && !r.authorization), 'nor the git server');
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

  // ---- push and pull, with the credentials of Iceberg's store

  // The repository of the pushes of this browser, its URL, the host of its
  // credentials in IceCredentialStore, and its class file in Tonel
  const pushName = 'push-' + t.name, pushRemote = `${server.git}${pushName}.git`;
  const gitHost = new URL(server.git).hostname, gitAuthority = new URL(server.git).host;
  const CLASS_FILE = 'src/GitSpecPushDemo/GitSpecPushDemo.class.st';
  seedRepository(root, pushName, FILES, 'first');
  // a method of GitSpecPushDemo committed on the server, as Tonel has it
  const serverMethod = (selector, value, message) =>
    commitTo(root, pushName, { [CLASS_FILE]: fileOf(root, pushName, CLASS_FILE) +
                               `\n{ #category : 'accessing' }\nGitSpecPushDemo >> ${selector} [\n\n\t^ ${value}\n]\n` }, message);
  // Smalltalk that answers 'pushed', or the error of the push (its quotes
  // made backquotes, for the regular expressions)
  const PUSH_REPO = '(Smalltalk at: #GitSpecPushRepo)';
  const tried = what => `[ ${what}. 'pushed' ] on: Error do: [ :e | ('error ' , e class name , ': ' , e messageText) ` +
    "copyReplaceAll: '''' with: '`' ]";
  const push = tried(`${PUSH_REPO} branch pushTo: ${PUSH_REPO} origin`);
  const PUSHED = /'pushed'|'error [^']*'/;
  // the credentials of IceCredentialStore for the git server: these, or none
  const storeCredentials = credentials =>
    `| s c | s := IceCredentialStore current. c := s plaintextCredentialForHostname: '${gitHost}'. ` +
    `c isPresent ifTrue: [ s removeCredential: c ]. ` +
    (credentials ? `s storeCredential: (IcePlaintextCredentials new username: '${credentials.user}'; ` +
                   `password: '${credentials.password}'; yourself) forHostname: '${gitHost}'. ` : '') +
    `'stored ' , (s plaintextCredentialForHostname: '${gitHost}') isPresent printString`;
  const receive = r => /git-receive-pack/.test(r.url);
  const CREDENTIALS_NOTE = 'The user name and token that ' + gitAuthority + ' asked for go through the git proxy ' +
    proxyOrigin + ', which can read them.';

  await check('a push through the site\'s default proxy: the transport refuses credentials before Iceberg is asked, the proxy gets none, and the notice offers that proxy', async () => {
    await page.click('#settings');
    await page.waitForSelector('#settings-dialog[open]', { timeout: 5000 });
    await page.click('#git-proxy-mode-default');
    await page.click('#settings-save');
    assert(await stored() === null && JSON.stringify((await spied()).sent.slice(-1)) === '[["gitProxy",null]]',
           'the default: ' + JSON.stringify((await spied()).sent));
    const none = await evalTo(storeCredentials(null), /'stored [^']*'/);
    assert(/'stored false'/.test(none), 'no credentials in the store: ' + none.trim().slice(-200));
    const made = await evalTo(`| r c | r := IceRepositoryCreator new remote: (IceGitRemote url: '${pushRemote}'); ` +
                              "location: FileLocator imageDirectory / 'git-spec' / 'push'; createRepository. r register. " +
                              'Smalltalk at: #GitSpecPushRepo put: r. ' +
                              "c := (Object << #GitSpecPushDemo) package: 'GitSpecPushDemo'; install. " +
                              "c compile: 'one ^ 1' classified: 'accessing'. r workingCopy addPackageNamed: 'GitSpecPushDemo'. " +
                              "'committed ' , (r commitWithMessage: 'pushed from the browser') comment , ', provider ' , " +
                              'IceCredentialsProvider providerType name',
                              /'committed [^']*'|'error [^']*'|Error/);
    assert(/'committed pushed from the browser, provider \w+CredentialsProvider'/.test(made),
           'a clone and a commit: ' + made.trim().slice(-300));
    proxyAt = server.log.proxy.length;
    const before = server.log.git.length;
    const out = await evalTo(push, PUSHED);
    assert(out.includes('asks for a user name and a password or token (401), which this page sends only through a git proxy that you chose') &&
           out.includes(proxyOrigin), 'the push: ' + out.trim().slice(-400));
    const asked = server.log.git.slice(before).filter(receive);
    assert(asked.length === 1 && asked[0].status === 401 && !asked[0].authorization && asked[0].proxied,
           'one request of the push, without credentials: ' + JSON.stringify(asked));
    assert(proxied().every(r => !r.authorization), 'no credentials went to the proxy: ' + JSON.stringify(proxied()));
    const events = (await spied()).events.filter(e => e.event === 'failed' && e.credentials === 'refused');
    assert(events.length === 1 && events[0].proxy === proxyOrigin, 'the worker said why: ' + JSON.stringify((await spied()).events.slice(-3)));
    const shown = await notice();
    assert(shown && shown.text.startsWith(gitAuthority + ' asks for a user name and token') && shown.text.includes(proxyOrigin) &&
           shown.action === 'Settings', 'the notice: ' + JSON.stringify(shown));
    const alt = await page.$eval('#notice-alt', e => e.hidden ? null : e.textContent);
    assert(alt === 'Use ' + proxyOrigin + ' for them', 'and its choice of the proxy: ' + alt);
    await evalTo('3 + 4', /7/);
  });

  await check('the notice makes that proxy My proxy: without stored credentials the push fails with the 401 of none given, with those of Iceberg\'s store it pushes', async () => {
    await page.click('#notice-alt');
    assert(await stored() === server.proxy, 'My proxy: ' + JSON.stringify(await stored()));
    assert(JSON.stringify((await spied()).sent.slice(-1)) === JSON.stringify([['gitProxy', server.proxy]]),
           'given to the worker: ' + JSON.stringify((await spied()).sent));
    proxyAt = server.log.proxy.length;
    let before = server.log.git.length;
    const none = await evalTo(push, PUSHED);
    assert(none.includes(`git http: ${gitAuthority} answered 401: `) && none.includes('none was given') &&
           none.includes(`${gitAuthority} says: No anonymous write access.`), 'without credentials: ' + none.trim().slice(-400));
    assert(server.log.git.slice(before).filter(receive).length === 1, 'one request: ' + JSON.stringify(server.log.git.slice(before)));
    const st = await evalTo(storeCredentials(PUSHER), /'stored [^']*'/);
    assert(/'stored true'/.test(st), 'stored: ' + st.trim().slice(-200));
    before = server.log.git.length;
    // (the worker posts at most one 'failed' every GIT_FAILED_MS, 2 s: the
    // push without credentials just posted one, which would hide another)
    await page.waitForTimeout(2500);
    const eventsAt = (await spied()).events.length;
    const out = await evalTo(push, PUSHED);
    assert(/'pushed'/.test(out), 'the push: ' + out.trim().slice(-400));
    const asked = server.log.git.slice(before).map(r => `${r.method} ${r.url} ${r.status}${r.authorization ? ' authorization' : ''}`);
    assert(JSON.stringify(asked) === JSON.stringify([
      `GET /${pushName}.git/info/refs?service=git-receive-pack 401`,
      `GET /${pushName}.git/info/refs?service=git-receive-pack 200 authorization`,
      `POST /${pushName}.git/git-receive-pack 200 authorization`]), 'a 401, then the same GET and the POST with credentials: ' + asked.join(', '));
    assert(server.log.git.slice(before).every(r => r.proxied), 'through the proxy');
    assert(proxied().every(r => !/git-spec-pusher|s3cret/.test(r.url)) && server.log.git.every(r => !/git-spec-pusher|s3cret/.test(r.url)),
           'no URL carried the credentials');
    const log = logOf(root, pushName);
    assert(log[0] === 'pushed from the browser (1)', 'the server\'s git: ' + JSON.stringify(log));
    const spy = await spied();
    assert(spy.events.filter(e => e.event === 'credentials').length === 1 && spy.notices.filter(n => n === CREDENTIALS_NOTE).length === 1 &&
           (await term()).split(CREDENTIALS_NOTE).length - 1 === 1, 'the credentials through the proxy, told once: ' +
           JSON.stringify(spy.notices.slice(-3)));
    const noted = await page.evaluate(() => localStorage.getItem('pharo-wasm.gitCredentialsNoted'));
    assert(noted === proxyOrigin, 'noted: ' + noted);
    assert(!spy.events.slice(eventsAt).some(e => e.event === 'failed'),
           'the 401 answered with credentials is no failure: ' + JSON.stringify(spy.events.slice(eventsAt)));
  });

  await check('pull: a fast-forward loads what the server got; with diverged branches the push is refused, the pull merges, and the merge pushes', async () => {
    serverMethod('two', 2, 'remote ff');
    const before = server.log.git.length;
    const ff = await evalTo(tried(`${PUSH_REPO} pull`).replace("'pushed'", `'pulled ' , ${PUSH_REPO} headCommit comment trimBoth , ` +
                            `' parents ' , ${PUSH_REPO} headCommit ancestors size printString , ' ' , ` +
                            `(Smalltalk at: #GitSpecPushDemo) selectors asSortedCollection asArray printString , ' modified ' , ` +
                            `${PUSH_REPO} workingCopy isModified printString`), /'pulled [^']*'|'error [^']*'/);
    assert(/'pulled remote ff parents 1 #\(#one #two\) modified false'/.test(ff), 'the fast-forward: ' + ff.trim().slice(-300));
    const local = await evalTo(`(Smalltalk at: #GitSpecPushDemo) compile: 'three ^ 3' classified: 'accessing'. ` +
                               `'committed ' , (${PUSH_REPO} commitWithMessage: 'local change') comment`, /'committed [^']*'/);
    assert(/'committed local change'/.test(local), 'a local commit: ' + local.trim().slice(-200));
    serverMethod('four', 4, 'remote div');
    const behind = await evalTo(push, PUSHED);
    assert(/'error IceRemoteDesynchronized/.test(behind), 'the push behind the server: ' + behind.trim().slice(-300));
    const merged = await evalTo(tried(`${PUSH_REPO} pull`).replace("'pushed'", `'pulled ' , (${PUSH_REPO} headCommit comment copyUpTo: $ ) , ` +
                                `' parents ' , ${PUSH_REPO} headCommit ancestors size printString , ' ' , ` +
                                `(Smalltalk at: #GitSpecPushDemo) selectors asSortedCollection asArray printString , ' modified ' , ` +
                                `${PUSH_REPO} workingCopy isModified printString`), /'pulled [^']*'|'error [^']*'/);
    assert(/'pulled Merge parents 2 #\(#four #one #three #two\) modified false'/.test(merged), 'the merge: ' + merged.trim().slice(-300));
    const out = await evalTo(push, PUSHED);
    assert(/'pushed'/.test(out), 'the push of the merge: ' + out.trim().slice(-300));
    const log = logOf(root, pushName);
    assert(/^Merge [0-9a-f]+ \(2\)$/.test(log[0]) && log.slice(1, 3).sort().join() === 'local change (1),remote div (1)',
           'the server\'s git: ' + JSON.stringify(log));
    assert(server.log.git.slice(before).every(r => r.proxied), 'all through the proxy');
    assert(trapped().length === 0, 'no failed callout: ' + trapped().join(' | '));
  });

  await check('a pull whose merge conflicts fails at once in the Console, with IceMergeAborted, and leaves the repository as it was', async () => {
    // (the server's #one answers 111, the local one 11)
    const text = fileOf(root, pushName, CLASS_FILE), changed = text.replace(/(>> one \[[\s\S]*?\^ )1(\s*\])/, '$1111$2');
    assert(changed !== text, 'the method #one in the class file of the server: ' + text.slice(0, 300));
    const local = await evalTo(`(Smalltalk at: #GitSpecPushDemo) compile: 'one ^ 11' classified: 'accessing'. ` +
                               `'committed ' , (${PUSH_REPO} commitWithMessage: 'local one') comment`, /'committed [^']*'/);
    assert(/'committed local one'/.test(local), 'a local commit: ' + local.trim().slice(-200));
    commitTo(root, pushName, { [CLASS_FILE]: changed }, 'remote one');
    const t0 = Date.now();
    const out = await evalTo(tried(`${PUSH_REPO} pull`).replace("'pushed'", "'pulled'"), /'pulled'|'error [^']*'/, 30000);
    const ms = Date.now() - t0;
    assert(/'error IceMergeAborted: [^']*the merge has conflicts, which Iceberg resolves in a dialog that only the world shows/.test(out),
           'the pull, in ' + ms + ' ms: ' + out.trim().slice(-400));
    const state = await evalTo(`'head ' , ${PUSH_REPO} headCommit comment trimBoth , ', one ' , (Smalltalk at: #GitSpecPushDemo) new one printString , ` +
                               `', incoming ' , (${PUSH_REPO} branch incomingCommitsFrom: ${PUSH_REPO} origin) size printString`, /'head [^']*'/);
    assert(/'head local one, one 11, incoming 1'/.test(state), 'as it was: ' + state.trim().slice(-300));
    await evalTo('3 + 4', /7/);
    console.log(`#   ${t.name}: the pull with a conflict failed in ${ms} ms`);
  });

  await check('credentials never go to another host: not after a redirect of the advertisement, and a redirect of a request with them through the proxy stops it', async () => {
    const mover = new URL(server.moved), moverHost = mover.hostname;
    const creds = (host, c) => storeCredentials(c).split(`'${gitHost}'`).join(`'${host}'`);
    const st = await evalTo(creds(moverHost, PUSHER), /'stored [^']*'/);
    assert(/'stored true'/.test(st), 'stored for ' + moverHost + ': ' + st.trim().slice(-200));
    await evalTo(storeCredentials(null), /'stored [^']*'/);
    const cloneTo = (dir, url, global) =>
      `[ | r c | r := IceRepositoryCreator new remote: (IceGitRemote url: '${url}'); location: FileLocator imageDirectory / 'git-spec' / '${dir}'; ` +
      `createRepository. r register. Smalltalk at: #${global} put: r. c := (Object << #${global}Demo) package: '${global}Demo'; install. ` +
      `c compile: 'one ^ 1'. r workingCopy addPackageNamed: '${global}Demo'. r commitWithMessage: '${dir}'. 'cloned' ] ` +
      `on: Error do: [ :e | 'error ' , e class name , ': ' , e messageText ]`;
    const pushOf = global => tried(`(Smalltalk at: #${global}) branch pushTo: (Smalltalk at: #${global}) origin`);
    const gitAt = server.log.git.length, moverAt = server.log.mover.length;
    const refused = `git http: ${mover.host} redirected the request to ${gitAuthority}, which asks for a user name and a password or token (401)`;
    // the advertisement redirected (301) to the git server, which asks
    for (const [dir, global] of [['moved-adv', 'GitSpecMovedAdv']]) {
      const made = await evalTo(cloneTo(dir, `${server.moved}adv/${pushName}.git`, global), /'cloned'|'error [^']*'/);
      assert(/'cloned'/.test(made), 'a clone of the mover: ' + made.trim().slice(-300));
      const out = await evalTo(pushOf(global), PUSHED);
      assert(out.includes(refused), 'a push whose advertisement goes to another host: ' + out.trim().slice(-400));
    }
    assert(server.log.git.slice(gitAt).every(r => !r.authorization), 'the git server got no credentials: ' +
           JSON.stringify(server.log.git.slice(gitAt)));
    // a request with them (after the 401 of the mover) redirected (302) to
    // the git server: through the proxy, the browser sends them there again
    const made = await evalTo(cloneTo('moved-auth', `${server.moved}auth/${pushName}.git`, 'GitSpecMovedAuth'), /'cloned'|'error [^']*'/);
    assert(/'cloned'/.test(made), 'a clone of the mover: ' + made.trim().slice(-300));
    const gitBefore = server.log.git.length, eventsAt = (await spied()).events.length;
    await page.waitForTimeout(2500);
    const out = await evalTo(pushOf('GitSpecMovedAuth'), PUSHED);
    assert(out.includes(`git http: ${mover.host} redirected a request that carried the user name and password or token to ${gitAuthority}`) &&
           out.includes('revoke that token'), 'the push, stopped: ' + out.trim().slice(-400));
    const asked = server.log.git.slice(gitBefore).filter(receive);
    assert(asked.length === 1 && asked[0].method === 'GET' && asked[0].authorization,
           'the git server got the one request that the browser followed, and nothing after it: ' + JSON.stringify(asked));
    assert(server.log.mover.slice(moverAt).filter(r => r.authorization && r.status === 302).length === 1,
           'the mover got them once, and redirected: ' + JSON.stringify(server.log.mover.slice(moverAt)));
    const spy = await spied();
    const said = spy.events.slice(eventsAt).filter(e => e.event === 'failed' && e.credentials === 'redirected');
    assert(said.length === 1 && said[0].redirectedTo === gitAuthority && said[0].host === mover.host,
           'the worker said so: ' + JSON.stringify(spy.events.slice(eventsAt)));
    const shown = await notice();
    assert(shown && shown.text.startsWith(`${mover.host} redirected a request with your user name and token to ${gitAuthority}`),
           'the notice: ' + JSON.stringify(shown));
    await evalTo(creds(moverHost, null), /'stored [^']*'/);
    await evalTo('3 + 4', /7/);
  });

  await check('without the routes of the tests, the browser asks the proxy whether it takes the Authorization (a CORS preflight), and a push with credentials goes', async () => {
    // (pw.mjs routes every request of its context, and Chromium then answers
    // the preflights itself: a context without routes sends them)
    const context = await t.browser.newContext({ viewport: { width: 1280, height: 800 } });
    try {
      await context.addInitScript(proxy => { try { localStorage.setItem('pharo-wasm.gitProxy', proxy); } catch (e) { /* none */ } },
                                  server.proxy);
      const other = await context.newPage();
      t.watch(other);
      other.on('dialog', d => d.accept());
      await other.goto(t.base + 'index.html');
      await other.waitForFunction(() => document.getElementById('status').dataset.state === 'waiting', null, { timeout: 60000 });
      const otherTerm = () => other.$eval('#term', e => e.textContent);
      const evalOther = async (text, re, timeout = 60000) => {
        const at = (await otherTerm()).length;
        await other.fill('#line', text);
        await other.press('#line', 'Enter');
        await other.waitForFunction(([src, a]) => new RegExp(src).test(document.getElementById('term').textContent.slice(a)),
                                    [re.source, at], { timeout });
        await other.waitForFunction(() => document.getElementById('status').dataset.state === 'waiting', null, { timeout });
        return (await otherTerm()).slice(at);
      };
      // (a user for the commit: this browser's image has none yet)
      const made = await evalOther("| r c | (FileLocator home / '.gitconfig') ensureDelete; writeStreamDo: [ :w | w nextPutAll: '[user]' , " +
                                   "String lf , String tab , 'name = Git Spec Preflight' , String lf , String tab , " +
                                   "'email = preflight@example.org' , String lf ]. " +
                                   `r := IceRepositoryCreator new remote: (IceGitRemote url: '${pushRemote}'); ` +
                                   "location: FileLocator imageDirectory / 'git-spec' / 'preflight'; createRepository. r register. " +
                                   'Smalltalk at: #GitSpecPreflightRepo put: r. ' +
                                   "c := (Object << #GitSpecPreflightDemo) package: 'GitSpecPreflightDemo'; install. c compile: 'one ^ 1'. " +
                                   "r workingCopy addPackageNamed: 'GitSpecPreflightDemo'. 'committed ' , (r commitWithMessage: 'preflight') comment",
                                   /'committed [^']*'|'error [^']*'|Error/);
      assert(/'committed preflight'/.test(made), 'a clone and a commit: ' + made.trim().slice(-300));
      await evalOther(storeCredentials(PUSHER), /'stored [^']*'/);
      const proxyAt0 = server.log.proxy.length;
      const out = await evalOther(tried('(Smalltalk at: #GitSpecPreflightRepo) branch pushTo: (Smalltalk at: #GitSpecPreflightRepo) origin'), PUSHED);
      assert(/'pushed'/.test(out), 'the push: ' + out.trim().slice(-400));
      const got = server.log.proxy.slice(proxyAt0);
      const first = got.findIndex(r => r.authorization);
      const preflight = got.findIndex(r => r.method === 'OPTIONS' && /(^|[ ,])authorization($|[ ,])/.test(r.preflight || ''));
      assert(preflight >= 0 && first > preflight && /git-receive-pack/.test(got[preflight].url),
             'a preflight that asks for the Authorization, before the first request with it: ' +
             JSON.stringify(got.map(r => [r.method, r.url.split('?')[1] || r.url.slice(-30), r.authorization, r.preflight || ''])));
      assert(logOf(root, pushName)[0] === 'preflight (1)', 'the server\'s git: ' + JSON.stringify(logOf(root, pushName)));
      await evalOther(storeCredentials(null), /'stored [^']*'/);
    } finally {
      await context.close();
    }
  });

  await check('in the world, a push without credentials fails at once, Iceberg\'s dialog opens after it, and what it is given pushes', async () => {
    // (the world boots with tests/wasm/st/git-world.st, whose evaluations
    // run in the UI process, as a click would)
    const evaluator = fs.readFileSync(path.join(path.dirname(new URL(import.meta.url).pathname), 'st', 'git-world.st'), 'utf8');
    const patch = `
;(function () {
  var D = self.PharoVMDriver, start = D.start, vmArgs = D.vmArgs, source = ${JSON.stringify(evaluator)};
  if (!D || D.gitWorld) return;
  D.gitWorld = true;
  D.vmArgs = function (mode) {
    var args = vmArgs.apply(this, arguments);
    return mode === 'world' ? args.concat(['st', '/pharo/st/git-world.st']) : args;
  };
  D.start = function (create, o) {
    o.files = (o.files || []).concat([{ path: '/pharo/st/git-world.st', data: source }]);
    return start.call(this, create, o);
  };
})();
`;
    const withEvaluator = route => route.fulfill({ contentType: 'text/javascript',
      body: fs.readFileSync(path.join(t.webDir, 'vm-driver.js'), 'utf8') + patch });
    await t.context.route('**/vm-driver.js*', withEvaluator);
    try {
      await page.goto(t.base + 'world.html');
      await page.waitForFunction(() => window.PharoWorld && window.PharoWorld.stats.frames > 0, null, { timeout: 120000 });
      let asked = 0;
      // (the worker answers the fs requests between the slices of the VM:
      // one that never ends, inside a callback, would leave them unanswered)
      const answered = (p, what) => Promise.race([p, new Promise((resolve, reject) =>
        setTimeout(() => reject(new Error('the worker did not answer ' + what + ' in 20 s: the VM is stuck in a slice')), 20000))]);
      // an evaluation in the UI process of the world: its printString
      const evalWorld = async (source, timeout = 60000) => {
        const n = ++asked;
        await answered(page.evaluate(src => window.PharoWorld.fs('writeFile', '/pharo/git-eval.st', { data: src }), source),
                       'the request ' + source.slice(0, 60));
        const t0 = Date.now();
        for (;;) {
          let text = '';
          try {
            text = await answered(page.evaluate(() => window.PharoWorld.readFile('/pharo/git-eval.out')
              .then(d => new TextDecoder().decode(d), () => '')), 'a read of its answer');
          } catch (e) {
            if (/did not answer/.test(e.message)) throw e;
          }
          const m = /^(\d+) ([\s\S]*)\n<end>\n$/.exec(text);
          if (m && Number(m[1]) === n) return m[2];
          if (Date.now() - t0 > timeout) throw new Error('no answer to ' + source.slice(0, 80) + ' in ' + timeout + ' ms; ' +
            'the page says ' + JSON.stringify(await page.evaluate(() => [window.PharoWorld.state, document.getElementById('notice-text').textContent])));
          await page.waitForTimeout(100);
        }
      };
      const empty = await evalWorld(storeCredentials(null));
      assert(empty === "'stored false'", 'no credentials in the store: ' + empty);
      const made = await evalWorld(`| r c | r := IceRepositoryCreator new remote: (IceGitRemote url: '${pushRemote}'); ` +
        "location: FileLocator imageDirectory / 'git-spec' / 'world-push'; createRepository. r register. " +
        'Smalltalk at: #GitSpecWorldRepo put: r. ' +
        "c := (Object << #GitSpecWorldDemo) package: 'GitSpecWorldDemo'; install. c compile: 'one ^ 1'. " +
        "r workingCopy addPackageNamed: 'GitSpecWorldDemo'. (r commitWithMessage: 'from the world') comment , ', ' , " +
        'IceCredentialsProvider providerType name');
      assert(made === "'from the world, OSWebCredentialsProvider'", 'a clone and a commit in the world: ' + made);
      const before = server.log.git.length, t0 = Date.now();
      const worldPush = tried('(Smalltalk at: #GitSpecWorldRepo) branch pushTo: (Smalltalk at: #GitSpecWorldRepo) origin');
      const none = await evalWorld(worldPush);
      const ms = Date.now() - t0;
      assert(none.includes(`git http: ${gitAuthority} answered 401: `) && none.includes('none was given'),
             'the push, which returned in ' + ms + ' ms: ' + none.slice(0, 400));
      assert(server.log.git.slice(before).filter(receive).length === 1, 'one request: ' + JSON.stringify(server.log.git.slice(before)));
      // the dialog, which the push asked for once it returned
      const LOGIN = `Please log in to ${gitHost}`;
      let windows = '';
      for (let i = 0; i < 50 && !windows.includes(LOGIN); i++)
        windows = await evalWorld('(World systemWindows collect: [ :w | w label asString ]) asArray');
      assert(windows.includes(LOGIN), 'Iceberg\'s dialog: ' + windows);
      assert(await page.evaluate(() => window.PharoWorld.state) !== 'busy', 'the world is not busy');
      const accepted = await evalWorld('| p | p := (IceTipAskForPlaintextCredentialsPresenter allSubInstances ' +
        `select: [ :each | each window notNil and: [ each window isOpen ] ]) first. ` +
        `p usernameInput text: '${PUSHER.user}'. p passwordInput text: '${PUSHER.password}'. ` +
        'p window triggerOkAction. p window close. #accepted');
      assert(accepted === '#accepted', 'its Ok: ' + accepted);
      let kept = '';
      for (let i = 0; i < 50 && !/stored true/.test(kept); i++)
        kept = await evalWorld(`'stored ' , (IceCredentialStore current plaintextCredentialForHostname: '${gitHost}') isPresent printString , ` +
                               "' pending ' , OSWebCredentialsProvider pendingHosts printString");
      assert(kept === "'stored true pending #()'", 'what the dialog was given is in Iceberg\'s store: ' + kept);
      const out = await evalWorld(worldPush);
      assert(out === "'pushed'", 'the push again: ' + out);
      const log = logOf(root, pushName);
      assert(log[0] === 'from the world (1)', 'the server\'s git: ' + JSON.stringify(log));
      assert(server.log.git.slice(before).every(r => r.proxied), 'through the proxy');
      console.log(`#   ${t.name}: in the world, the push without credentials returned in ${ms} ms`);
    } finally {
      await t.context.unroute('**/vm-driver.js*', withEvaluator);
    }
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
