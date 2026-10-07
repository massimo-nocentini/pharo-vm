// page.js - the Console page (index.html) of the Pharo VM for WebAssembly
//
// The page side of the REPL: owns the VM's worker (vm-worker.js, see there
// for the message protocol), renders its output, and handles input,
// history, Stop, Save, Download, Open, Reset and the theme.  No
// dependencies but open-image.js.
//
// - pharo-web.wasm is compiled once here and handed to every worker, so
//   Restart only instantiates it.  A generation number per worker drops
//   the messages of a worker that was replaced.
// - Output is buffered and rendered once per animation frame, then acked
//   to the worker (which pauses the VM above 1 MiB unacked).  A hidden page
//   gets no animation frames: it acks the output as it comes, and keeps no
//   more of it for the next frame than the terminal would show.
// - Stop posts "interrupt": the REPL (st/web-repl.st) stops the evaluation.
//   When no evaluation could be stopped (the worker answers that the image
//   registered no interrupt semaphore, after the REPL's first prompt), or
//   when the worker then stays silent for 3 s (a long primitive that never
//   yields), it is terminated and a fresh one started.
// - The worker keeps the image in this browser whenever the image is saved,
//   by Save or otherwise, and boots it the next time; Reset forgets it.
//   Another tab may save over it: this one says so, and Save keeps it again.
//   An opened image is kept once it has started (its first prompt): one
//   that crashes or quits before is not, and Restart boots the saved image
//   again.  When the saved image itself does not start, the notice offers
//   Reset next to Restart.
// - Open (or files dropped on the page) starts an image of your own: a
//   Pharo .zip, or an .image, its .changes and its .sources, which
//   open-image.js chooses, unpacks and checks while the VM goes on, the
//   notice saying how far it got.  Only then is the VM replaced: a file it
//   cannot open leaves the session as it was.  The image's own .sources
//   goes with it (the worker keeps it with the image), else it gets the
//   one of the site.
// - The world page boots the saved image: when something was evaluated
//   since it was saved, its link offers to Save first.  While a save is
//   being stored, or code was changed that the saved image has not (the
//   worker says "edited"), the browser asks before the page goes.
// - The status pill is no live region: the one next to it says how the VM
//   goes (loading, starting, ready, ended), not how far the loading got, nor
//   the state of every evaluation, which the terminal, a log, tells.
// - Settings, shown when the VM has the smart-HTTP transport of libgit2
//   (ready.gitHttp), holds the CORS proxy of git's requests: a URL prefix
//   that the requests of Iceberg's https:// remotes go to, followed by the
//   host and the path of the repository (src/emscripten/gitSupport.c).  It
//   is kept in localStorage (pharo-wasm.gitProxy, which world.js and sdl.js
//   read too) and goes to the worker in init.gitProxy, and to a running VM
//   at once ("gitProxy"), also when another tab changes it; the worker makes
//   it Module.gitHttpProxy.  It is set only in the dialog, never from the URL:
//   a link must not send someone's git traffic, code and credentials, to a
//   proxy of its choosing.  The dialog and a note at the start name the
//   origin that the requests go to.
// - History, the theme and the proxy live in localStorage when available;
//   everything works without it (the proxy then only for this session).
// - The Notebook tab (notebook.js) runs a second VM, in a worker of its own
//   (vm-worker.js in its notebook mode), which boots the saved image without
//   ever storing one.  window.PharoPage gives it what the page shares: the
//   storage, the compiled .wasm, the proxy, Settings and the theme; the page
//   sends it "pharo:tab", "pharo:settings", "pharo:theme" and
//   "pharo:image-changed" (the saved image was saved, reset, replaced by
//   another tab, or is an opened one now) as CustomEvents on window.  The
//   tab shown is kept (pharo-wasm.tab, as the theme for every site); the
//   Console's keys (Esc, Ctrl+C, Ctrl+L) and the drops that open an image
//   act only while its tab shows.

(function () {
  'use strict';

  const RUNNING = 0, WAITING = 1, BUSY = 2, SLEEPING = 4;
  const TERM_LINES = 20000;             // terminal scrollback cap,
  const TERM_CHARS = 2000000;           // also for output without newlines
  const HISTORY_MAX = 500;
  const WATCHDOG_MS = 3000;
  const PROMPT = 'st> ';
  const AT_PROMPT = /(^|\n)st> $/;
  const SAVE = 'Smalltalk snapshot: true andQuit: false';
  const BOOTSTRAP = "CodeImporter evaluateFileNamed: '/pharo/st/web-bootstrap.st'";

  const BUILD = (document.querySelector('meta[name="pharo-build"]') || {}).content || '';
  const Q = BUILD && BUILD.indexOf('@') < 0 ? '?v=' + encodeURIComponent(BUILD) : '';
  const $ = id => document.getElementById(id);
  const term = $('term'), line = $('line'), statusPill = $('status'), statusText = $('status-text');
  const consoleVisible = () => !$('panel-console').hidden;     // its tab shows

  // ---- storage (may be unavailable: private mode, blocked site data)

  const PREFIX = 'pharo-wasm.';
  const store = {
    prefix: PREFIX,                     // of the keys in localStorage (and in storage events)
    get(k, d) {
      try { const v = localStorage.getItem(PREFIX + k); return v === null ? d : v; }
      catch (e) { return d; }
    },
    // (whether it could)
    set(k, v) {
      try { localStorage.setItem(PREFIX + k, v); return true; } catch (e) { return false; }
    },
    remove(k) {
      try { localStorage.removeItem(PREFIX + k); return true; } catch (e) { return false; }
    },
  };
  const touch = matchMedia('(pointer: coarse)').matches;
  // The key of name for this site alone: the sites of an origin are its
  // directories, as for the saved image (vm-storage.js), so that the
  // notebooks of /stable/ and /preview/ do not overwrite each other
  function siteKey(name) {
    let dir = '/';
    try { dir = new URL('.', location.href).pathname; } catch (e) { /* no URL: the root */ }
    return name + ':' + dir;
  }

  const THEMES = { auto: 'System', light: 'Light', dark: 'Dark' };
  let theme = store.get('theme', 'auto');
  if (!(theme in THEMES)) theme = 'auto';
  function applyTheme() {
    if (theme === 'light' || theme === 'dark') document.documentElement.dataset.theme = theme;
    else delete document.documentElement.dataset.theme;
    $('theme-label').textContent = THEMES[theme];
    $('theme').setAttribute('aria-label', 'Theme: ' + THEMES[theme]);
    $('theme').title = 'Theme: ' + (theme === 'auto' ? 'follow the system' : THEMES[theme].toLowerCase());
  }
  applyTheme();
  // System, Light, Dark, and round again (the theme button, and the
  // notebook's menu)
  function cycleTheme() {
    theme = theme === 'auto' ? 'light' : theme === 'light' ? 'dark' : 'auto';
    store.set('theme', theme);
    applyTheme();
    dispatchEvent(new CustomEvent('pharo:theme', { detail: { theme } }));
    return theme;
  }

  // The proxy of git's HTTP requests, as typed: {value: ''} for none, else
  // {value, origin} for an http: or https: URL without credentials or
  // fragment (a bare origin gets its /, which the host of the repository
  // follows), else {error}.  world.js and sdl.js check what they read in
  // the same way.
  function parseGitProxy(text) {
    const s = String(text || '').trim();
    if (!s) return { value: '' };
    let u;
    try { u = new URL(s); } catch (e) { return { error: 'This is not a URL.' }; }
    if (u.protocol !== 'http:' && u.protocol !== 'https:') return { error: 'The proxy must be an http:// or https:// URL.' };
    if (u.username || u.password) return { error: 'Give the URL of the proxy without a user name or password.' };
    if (u.hash || s.indexOf('#') >= 0) return { error: 'The URL of the proxy cannot have a #fragment.' };
    return { value: u.pathname === '/' && !u.search ? u.origin + '/' : s, origin: u.origin };
  }
  // the stored one, '' when none or not a valid one
  let gitProxy = parseGitProxy(store.get('gitProxy', '')).value || '';

  // ---- terminal output

  let pending = [];                     // [{cls, text, lineStart}] not yet rendered
  let pendingChars = 0;                 // and their characters
  let ackChars = 0;                     // worker output rendered, not yet acked
  let rendering = 0;
  let lines = 0, chars = 0;
  let tail = '';                        // the end of the transcript, for prompt detection

  function countLines(s) {
    let n = 0;
    for (let i = s.indexOf('\n'); i >= 0; i = s.indexOf('\n', i + 1)) n++;
    return n;
  }

  // cls: 'out' (stdout), 'err', 'in' (echoed input), 'sys' (page notes)
  function emit(cls, text, fromWorker) {
    if (!text) return;
    const lineStart = !tail || tail.endsWith('\n');
    if (cls !== 'sys') tail = (tail + text).slice(-64);
    const last = pending[pending.length - 1];
    if (last && last.cls === cls) last.text += text; else pending.push({ cls, text, lineStart });
    pendingChars += text.length;
    if (fromWorker) ackChars += text.length;
    // a hidden page gets no frames: ack now, and keep what the terminal
    // would show of it
    if (document.hidden) {
      ack();
      if (pendingChars > TERM_CHARS + 65536) trimPending();
    }
    if (!rendering) rendering = requestAnimationFrame(render);
  }
  function ack() {
    if (ackChars && worker) worker.postMessage({ type: 'ack', chars: ackChars });
    ackChars = 0;
  }
  // Drop the oldest output not rendered yet, beyond TERM_CHARS
  function trimPending() {
    let excess = pendingChars - TERM_CHARS;
    while (excess > 0 && pending.length) {
      const first = pending[0];
      if (first.text.length <= excess && pending.length > 1) {
        pending.shift();
        pendingChars -= first.text.length;
        excess -= first.text.length;
        continue;
      }
      const cut = Math.min(excess, first.text.length);
      first.lineStart = first.text[cut - 1] === '\n';
      first.text = first.text.slice(cut);
      pendingChars -= cut;
      excess = 0;
    }
  }
  // a page message on a line of its own, as a Smalltalk comment
  function note(text) {
    const lastText = pending.length ? pending[pending.length - 1].text : term.textContent;
    emit('sys', (lastText && !lastText.endsWith('\n') ? '\n' : '') + '"' + text + '"\n');
  }

  // stdout, with the REPL's prompts (st> at the start of a line) dimmed
  function outNode(text, lineStart) {
    const span = document.createElement('span');
    const add = (s, cls) => {
      if (!s) return;
      if (!cls) { span.appendChild(document.createTextNode(s)); return; }
      const p = document.createElement('span');
      p.className = cls;
      p.textContent = s;
      span.appendChild(p);
    };
    let from = 0, at = lineStart ? 0 : text.indexOf('\n') + 1 || -1;
    while (at >= 0 && at < text.length) {
      if (text.startsWith(PROMPT, at)) {
        add(text.slice(from, at));
        add(PROMPT, 'p');
        from = at + PROMPT.length;
      }
      const nl = text.indexOf('\n', at);
      at = nl < 0 ? -1 : nl + 1;
    }
    add(text.slice(from));
    return span;
  }

  function render() {
    rendering = 0;
    const loading = $('loading');
    if (loading && pending.length) loading.remove();
    const stick = term.scrollHeight - term.scrollTop - term.clientHeight < 40;
    const frag = document.createDocumentFragment();
    for (const { cls, text, lineStart } of pending) {
      let node;
      if (cls === 'out') node = outNode(text, lineStart);
      else {
        node = document.createElement('span');
        node.className = cls;
        node.textContent = text;
      }
      node._lines = countLines(text);
      node._chars = text.length;
      lines += node._lines;
      chars += text.length;
      frag.appendChild(node);
    }
    pending = [];
    pendingChars = 0;
    term.appendChild(frag);
    trim();
    if (stick) term.scrollTop = term.scrollHeight;
    ack();
  }

  function trim() {
    let excess = lines - TERM_LINES;
    while (excess > 0 && term.firstChild) {
      const n = term.firstChild, nl = n._lines || 0;
      if (nl <= excess) {
        term.removeChild(n);
        lines -= nl;
        chars -= n._chars || 0;
        excess -= nl;
        continue;
      }
      const t = n.textContent;
      let i = -1;
      for (let k = 0; k < excess; k++) i = t.indexOf('\n', i + 1);
      n.textContent = t.slice(i + 1);
      n._lines -= excess;
      lines -= excess;
      chars -= i + 1;
      n._chars -= i + 1;
      excess = 0;
    }
    let cexcess = chars - TERM_CHARS;
    while (cexcess > 0 && term.firstChild) {
      const n = term.firstChild, nc = n._chars || 0, nl = n._lines || 0;
      if (nc <= cexcess) {
        term.removeChild(n);
        chars -= nc;
        lines -= nl;
        cexcess -= nc;
        continue;
      }
      const t = n.textContent.slice(cexcess);
      n.textContent = t;
      n._chars = t.length;
      n._lines = countLines(t);
      lines -= nl - n._lines;
      chars -= cexcess;
      cexcess = 0;
    }
  }

  // Empties the terminal, but for the prompt that the REPL waits at
  function clearTerm() {
    const prompt = atPrompt();
    pending = [];
    pendingChars = 0;
    term.textContent = '';
    lines = chars = 0;
    ack();
    if (prompt) {
      tail = '';
      emit('out', PROMPT);
    }
  }

  // ---- status

  let worker = null, gen = 0, ready = false, alive = false, prompted = false, state = null;
  let lastMsgAt = 0, watchdog = 0, eofSent = false, loadedText = 'Loading';
  let world = false, prepared = false, unavailable = null;
  let persisted = false;                // an image is saved in this browser (ready, saved, Reset)
  // what the slot lacks: input evaluated since this VM started or was saved,
  // code changed (the worker's "edited"), a save not stored yet
  let evaluated = false, edited = false, storing = false;
  let afterSave = null;                 // what to do once the next save is stored
  let leaving = false;                  // the page goes, and asked first
  let upload = null;                    // {name, image, changes, sources} to boot until the worker keeps it
  let opening = false;                  // Open unpacks files
  let source = null;                    // where the image of the worker came from (ready)
  let gitHttp = false;                  // the VM has libgit2's smart-HTTP transport (ready)
  const reqs = new Map();
  let reqId = 0;

  const atPrompt = () => AT_PROMPT.test(tail);

  function setStatus(key, text) {
    statusPill.dataset.state = key;
    if (statusText.textContent !== text) statusText.textContent = text;
    if (key === 'loading') say('Loading Pharo');
    else if (key === 'starting') say('Starting Pharo');
    else if (key === 'waiting' && said === 'Starting Pharo') say('Pharo is ready');
    else if (key === 'exited' || key === 'crashed' || key === 'error') say(text);
  }
  // The live region, see above
  let said = '';
  function say(text) {
    if (text !== said) $('status-live').textContent = said = text;
  }
  function updateStatus() {
    if (!alive) return;
    if (!ready) return setStatus('loading', loadedText);
    if (!prompted) return setStatus('starting', 'Starting');
    switch (state) {
    case WAITING:  return setStatus('waiting', 'Waiting for input');
    case BUSY:     return setStatus('busy', 'Busy');
    case SLEEPING: return setStatus('sleeping', 'Sleeping');
    default:       return setStatus('running', 'Running');
    }
  }
  function updateControls() {
    const on = !!worker && ready && alive;
    line.disabled = !on;
    for (const id of ['send', 'stop', 'save']) $(id).disabled = !on;
    $('download').disabled = !(worker && ready);
    $('reset').disabled = !(worker && persisted);
    $('open').disabled = !!unavailable || opening;
    $('restart').disabled = !!unavailable;
    $('world').hidden = !(world && prepared);
    $('settings').hidden = !gitHttp;
    const nbSettings = document.querySelector('#nb-menu [data-act="settings"]');
    if (nbSettings) nbSettings.hidden = !gitHttp;
  }

  // The notice under the terminal: text, a button doing action, if any, and
  // a second one, alt {label, action}, if any
  let noticeAction = null, noticeAlt = null, noticeTimer = 0;
  function showNotice(text, label, action, ms, alt) {
    clearTimeout(noticeTimer);
    $('notice-text').textContent = text;
    $('notice-action').hidden = !label;
    $('notice-action').textContent = label || '';
    noticeAction = action || null;
    $('notice-alt').hidden = !alt;
    $('notice-alt').textContent = alt ? alt.label : '';
    noticeAlt = alt ? alt.action : null;
    $('notice').hidden = false;
    if (ms) noticeTimer = setTimeout(hideNotice, ms);
  }
  function hideNotice() {
    clearTimeout(noticeTimer);
    $('notice').hidden = true;
    noticeAction = noticeAlt = null;
  }

  const mb = n => (n / 1048576).toFixed(1) + ' MB';

  // ---- the VM's worker

  let wasmP = null, noMemory64 = false;
  function compileWasm(url) {
    return (async () => {
      if (WebAssembly.compileStreaming) {
        try { return await WebAssembly.compileStreaming(fetch(url)); }
        catch (e) { /* no application/wasm type: fall back */ }
      }
      const r = await fetch(url);
      if (!r.ok) throw new Error('cannot fetch ' + url.replace(/\?.*/, '') + ' (HTTP ' + r.status + ')');
      return WebAssembly.compile(await r.arrayBuffer());
    })();
  }

  // A 64-bit memory: a build with WASM_WEB_MEMORY64=1 needs it
  function hasMemory64() {
    try {
      return WebAssembly.validate(new Uint8Array([
        0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00,   // magic, version
        0x05, 0x03, 0x01, 0x04, 0x00]));                  // memory: i64, min 0
    } catch (e) { return false; }
  }

  // pharo-web.wasm, compiled once for every worker of the page: the
  // Console's, and the notebook's (PharoPage.wasmModule).  A failure is
  // forgotten, and the next call fetches it again; in a browser without
  // memory64 it is that of a build that needs it (WASM_WEB_MEMORY64=1),
  // and no VM can run here (PharoPage.unavailable).
  function wasmModule() {
    if (!wasmP) {
      const p = wasmP = compileWasm('pharo-web.wasm' + Q);
      p.catch(() => {
        if (wasmP === p) wasmP = null;
        if (!hasMemory64()) noMemory64 = true;
      });
    }
    return wasmP;
  }

  const HTTP_HINT = 'This page must be served over HTTP, for example with "make wasm-serve".';
  const NO_MEMORY64 = 'This browser has no 64-bit WebAssembly memory (memory64), which this build needs.';

  function kill() {
    gen++;
    clearTimeout(watchdog);
    if (worker) worker.terminate();
    worker = null;
    alive = ready = false;
    state = null;
    evaluated = edited = storing = false;
    afterSave = null;
    for (const r of reqs.values()) r.reject(new Error('the VM was restarted'));
    reqs.clear();
    ackChars = 0;
  }

  function fatal(msg) {
    kill();
    setStatus('error', 'Unavailable');
    note(msg);
    updateControls();
    showNotice(msg.split('\n')[0], unavailable ? null : 'Restart', unavailable ? null : restart);
  }

  async function spawn() {
    kill();
    const g = gen;
    alive = true;
    eofSent = prompted = false;
    source = null;
    tail = '';
    loadedText = 'Loading';
    hideNotice();
    updateControls();
    updateStatus();
    let mod;
    try {
      mod = await wasmModule();
    } catch (e) {
      if (g === gen)
        fatal('Could not load pharo-web.wasm: ' + ((e && e.message) || e) + '\n' + (hasMemory64() ? HTTP_HINT : NO_MEMORY64));
      return;
    }
    if (g !== gen) return;
    let w;
    try { w = new Worker('vm-worker.js' + Q); }
    catch (e) { fatal('Could not start the VM worker: ' + ((e && e.message) || e) + '\n' + HTTP_HINT); return; }
    worker = w;
    w.onmessage = e => { if (g === gen) onWorker(e.data); };
    w.onerror = e => {
      if (g !== gen) return;
      e.preventDefault();
      console.error('vm-worker.js: ' + (e.message || 'failed to load'));
      fatal('The VM worker failed: ' + ((e && e.message) || 'could not load vm-worker.js') + '\n' + HTTP_HINT);
    };
    w.onmessageerror = () => { if (g === gen) fatal('A message of the VM worker could not be decoded.'); };
    w.postMessage({
      type: 'init',
      wasmModule: mod,
      manifestUrl: 'manifest.json' + Q,
      build: BUILD,
      mode: 'console',
      persist: true,
      upload: upload || undefined,
      gitProxy,
    });
  }

  // The VM ended; the worker stays, for Download and Reset.  Before its
  // first prompt (and not at the end of the input) its image did not start:
  // an upload is not kept, so Restart boots the saved image, or the
  // original; a saved image may be damaged, and Reset brings the original.
  function ended(status, text, notice) {
    render();
    clearTimeout(watchdog);
    alive = false;
    setStatus(status, text);
    updateControls();
    let alt;
    if (!prompted && !eofSent && source === 'upload') {
      upload = null;
      notice = 'The opened image did not start; it was not kept.';
    } else if (!prompted && !eofSent && source === 'saved') {
      notice = 'The image saved in this browser did not start.';
      alt = { label: 'Reset saved image', action: resetSaved };
    }
    showNotice(notice, 'Restart', restart, 0, alt);
  }

  function onReady(m) {
    ready = true;
    world = m.world;
    prepared = m.prepared;
    persisted = m.persisted;
    source = m.source;
    if (m.source === 'saved')
      note('Started the image saved in this browser on ' + new Date(m.savedAt).toLocaleString() + '.');
    else if (m.source === 'upload')
      note('Started the opened image, ' + (upload ? upload.name : m.image) +
           (upload && !upload.sources && m.sources ? ', with the .sources of this site, ' + m.sources : '') + '.');
    if (m.storageError) note('Note: ' + m.storageError + '.');
    gitHttp = !!m.gitHttp;
    if (gitHttp && gitProxy) note('Git requests go through the proxy at ' + parseGitProxy(gitProxy).origin + ' (Settings).');
    if (world && !prepared)
      showNotice('This image cannot open the Pharo world yet.', 'Prepare for the world', prepareWorld);
    updateControls();
    if (!touch && document.activeElement !== line && consoleVisible()) line.focus();
    updateStatus();
  }

  function onWorker(m) {
    lastMsgAt = performance.now();
    switch (m.type) {
    case 'progress': {
      const pct = (m.total ? Math.floor(100 * m.loaded / m.total) : 100) + '%';
      loadedText = m.phase === 'boot' ? 'Starting' : 'Loading ' + pct;
      const loading = $('loading');
      if (loading) {
        loading.textContent = m.phase === 'boot' ? 'Starting Pharo…'
          : (m.phase === 'restore' ? 'Restoring the saved image… ' : 'Downloading Pharo… ') + pct;
      }
      updateStatus();
      break;
    }
    case 'ready':
      onReady(m);
      break;
    case 'output':
      emit(m.fd === 2 ? 'err' : 'out', m.text, true);
      if (!prompted && atPrompt()) { prompted = true; updateStatus(); }
      break;
    case 'state':
      state = m.state;
      updateStatus();
      break;
    case 'interrupted':
      // nothing to stop it with: start afresh.  Before the first prompt the
      // REPL has registered nothing yet, and the watchdog covers a hung boot.
      if (!m.registered && alive && prompted && state !== WAITING) respawn();
      break;
    case 'storing':
      storing = true;
      break;
    case 'edited':
      edited = m.edited;
      break;
    case 'saved': {
      // the upload that this worker booted (m.upload), or an image it saved
      storing = false;
      const then = afterSave;
      afterSave = null;
      if (m.error) {
        showNotice((m.upload ? 'The opened image runs, but ' : 'Saved, but ') + m.error + '.', 'Download', download);
      } else {
        upload = null;                  // the slot holds this VM's image now
        persisted = true;
        // the REPL said, which the bytes of an upload may not have told
        const unprepared = world && prepared && !m.prepared;
        prepared = m.prepared;
        if (!m.upload) {
          evaluated = false;
          showNotice('The image is saved in this browser (' + mb(m.bytes) + ').', null, null, 6000);
        } else if (unprepared)
          showNotice('This image cannot open the Pharo world yet.', 'Prepare for the world', prepareWorld);
        else if ($('notice').hidden)    // e.g. not over the offer to prepare it for the world
          showNotice('The opened image is now kept in this browser.', null, null, 6000);
        imageChanged(m.upload ? 'opened' : 'saved');
        if (then && !m.upload) then();
      }
      updateControls();
      break;
    }
    case 'superseded':
      showNotice('Another tab has replaced the image saved in this browser. Save to keep this session instead.',
                 'Save', save);
      imageChanged('superseded');
      break;
    case 'exit':
      if (eofSent && m.code === 0) ended('exited', 'Exited (0)', 'The session ended.');
      else ended('exited', 'Exited (' + m.code + ')', 'Pharo quit with exit code ' + m.code + '.');
      note('Pharo quit (exit code ' + m.code + ')');
      break;
    case 'crash':
      emit('err', String(m.message).replace(/\n?$/, '\n') + (m.stacks ? m.stacks.replace(/\n?$/, '\n') : ''));
      ended('crashed', 'Crashed', 'The VM crashed.');
      note('The VM crashed');
      break;
    case 'fs-result': case 'reset': case 'file': case 'error': {
      const r = reqs.get(m.id);
      if (!r) break;
      if (m.type === 'file') {           // one message per file
        r.files.push(m);
        if (r.files.length < m.count) break;
      }
      reqs.delete(m.id);
      if (m.type === 'error') r.reject(new Error(m.message));
      else if (m.type === 'reset' && m.error) r.reject(new Error(m.error));
      else r.resolve(m.type === 'file' ? r.files : m);
      break;
    }
    }
  }

  // The image saved in this browser, which the notebook boots, is another
  // one now
  function imageChanged(reason) {
    dispatchEvent(new CustomEvent('pharo:image-changed', { detail: { reason } }));
  }

  function request(msg) {
    return new Promise((resolve, reject) => {
      if (!worker) { reject(new Error('the VM is not running')); return; }
      const id = ++reqId;
      reqs.set(id, { resolve, reject, files: [] });
      worker.postMessage(Object.assign({ id }, msg));
    });
  }

  // ---- Stop, Restart, EOF

  function respawn() {
    if (!prompted && source === 'upload') upload = null;   // it hung before its first prompt
    note('VM restarted (unsaved changes lost)');
    spawn();
  }

  function stop() {
    if (!worker || !alive || !ready) return;
    worker.postMessage({ type: 'interrupt' });
    if (state === WAITING) { line.value = ''; autosize(); }
    if (state === RUNNING || state === BUSY || !prompted) {
      const g = gen, t0 = performance.now();
      clearTimeout(watchdog);
      watchdog = setTimeout(() => {
        if (g !== gen || lastMsgAt > t0 || !alive) return;
        respawn();
      }, WATCHDOG_MS);
    }
  }

  function restart() {
    render();
    if (term.textContent) note('Restarting…');
    spawn();
  }

  function sendEof() {
    if (!worker || !alive || !ready) return;
    if (!confirm('End the session? Pharo quits without saving.')) return;
    eofSent = true;
    worker.postMessage({ type: 'eof' });
  }

  // ---- Save, Download, Open, Reset, and the world

  function echo(text) {
    emit('in', text + '\n');
    render();
    term.scrollTop = term.scrollHeight;
    state = RUNNING;
    updateStatus();
  }

  function save() {
    if (!worker || !alive || !ready) return;
    echo(SAVE);
    worker.postMessage({ type: 'save' });
  }

  // A download has long begun when its Blob URL is revoked: then the Blob,
  // of the size of the image, may go
  const REVOKE_MS = 60000;
  async function download() {
    if (!worker || !ready) return;
    let files;
    try { files = await request({ type: 'download' }); }
    catch (e) { note('Download failed: ' + e.message); return; }
    for (const f of files) {
      const url = URL.createObjectURL(new Blob([f.data], { type: 'application/octet-stream' }));
      setTimeout(() => URL.revokeObjectURL(url), REVOKE_MS);
      const a = document.createElement('a');
      a.href = url;
      a.download = f.name;
      a.hidden = true;
      document.body.appendChild(a);
      a.click();
      a.remove();
    }
    note('Downloaded ' + files.map(f => f.name + ' (' + mb(f.data.length) + ')').join(' and ') + '.');
  }

  // Open the files chosen or dropped (open-image.js); the VM is replaced
  // only once they are unpacked and checked.  The progress goes to the
  // notice, which is kept quiet meanwhile (aria-busy): the live region says
  // what is unpacked, once.
  async function openFiles(files) {
    if (unavailable || opening || !files.length) return;
    opening = true;
    updateControls();
    let opened = null, what = '';
    try {
      const choice = await PharoOpen.choose(files);
      if (persisted && !confirm('Replace the image saved in this browser with ' + choice.name + '?')) return;
      let shown = -1;
      const progress = (loaded, total) => {
        const pct = Math.floor(100 * loaded / Math.max(1, total));
        if (pct !== shown) showNotice('Unpacking ' + choice.from + '… ' + (shown = pct) + '%');
      };
      if (choice.from) {
        progress(0, 1);
        $('notice').setAttribute('aria-busy', 'true');
        say('Unpacking ' + choice.from);
      }
      opened = await PharoOpen.load(choice, progress);
      what = PharoOpen.describe(choice);
    } catch (e) {
      const text = String((e && e.message) || e);
      note('Could not open it: ' + text);
      showNotice(text, 'Open another', chooseFile);
    } finally {
      opening = false;
      $('notice').removeAttribute('aria-busy');
      updateControls();
    }
    if (!opened) return;
    hideNotice();
    upload = opened;
    render();
    note('Starting ' + what + '…');
    spawn();
  }
  const chooseFile = () => $('open-file').click();
  // The drops of the Console's tab.  Its handlers go on the capture phase
  // of window, ahead of the notebook's on its panel (which upload what is
  // dropped there, and may stop it): they see every drag, so that it is
  // counted out as it was counted in, but they show the overlay and open
  // only while the Console shows.  Over the notebook's panel, whether a
  // drop is taken (dragover) is the notebook's.
  const overNotebook = e => !consoleVisible() && $('panel-notebook').contains(e.target);
  PharoOpen.drops({
    addEventListener: (type, f) =>
      addEventListener(type, type === 'dragover' ? e => { if (!overNotebook(e)) f(e); } : f, true),
  }, {
    enabled: () => consoleVisible() && !$('open').disabled,
    show: on => { $('drop').hidden = !(on && consoleVisible()); },
    open: openFiles,
  });

  async function resetSaved() {
    if (!worker) return;
    if (!confirm('Delete the image saved in this browser, and start the original one again?')) return;
    try { await request({ type: 'resetStorage' }); }
    catch (e) { note('Reset failed: ' + e.message); return; }
    upload = null;
    persisted = false;
    imageChanged('reset');
    render();
    note('Deleted the image saved in this browser.');
    restart();
  }

  // File in OSWindow-Web (st/web-bootstrap.st), then save: the world page
  // boots the saved image
  function prepareWorld() {
    if (!worker || !alive || !ready) return;
    hideNotice();
    echo(BOOTSTRAP);
    worker.postMessage({ type: 'input', text: BOOTSTRAP + '\n' });
    save();
  }

  // The world page boots the saved image: offer to save first what this
  // VM did since
  const lacking = () => !!worker && alive && ready && (evaluated || edited || storing);
  function openWorld() {
    leaving = true;
    location.assign($('world').href);
  }
  $('world').addEventListener('click', e => {
    if (!lacking() || e.button !== 0 || e.ctrlKey || e.metaKey || e.shiftKey || e.altKey) return;
    e.preventDefault();
    showNotice('The world starts from the image saved in this browser, which lacks what you did here since it was saved.',
               'Save, then open the world', () => { afterSave = openWorld; save(); }, 0,
               { label: 'Open it without saving', action: openWorld });
  });
  addEventListener('beforeunload', e => {
    if (leaving || !worker || !(storing || (alive && edited))) return;
    e.preventDefault();
    e.returnValue = '';                 // older browsers ask for it
  });

  // ---- Settings: the proxy of git's HTTP requests

  const dialog = $('settings-dialog'), proxyInput = $('git-proxy'), proxyState = $('git-proxy-state');
  // what the dialog says of the text in the field
  function showProxyState() {
    const p = parseGitProxy(proxyInput.value);
    proxyInput.setAttribute('aria-invalid', p.error ? 'true' : 'false');
    proxyState.dataset.kind = p.error ? 'bad' : p.value ? 'proxy' : 'none';
    proxyState.textContent = p.error ||
      (p.value ? 'Git requests go to ' + p.origin + '.' : 'No proxy: git requests go to the repository itself.');
    return p;
  }
  function openSettings() {
    if (dialog.open) return;
    proxyInput.value = gitProxy;
    showProxyState();
    dialog.showModal();
    proxyInput.focus();
    proxyInput.select();
  }
  // Keep the proxy, and give it to the running VM: its next request goes there
  function saveSettings() {
    const p = showProxyState();
    if (p.error) { proxyInput.focus(); return; }
    dialog.close();
    if (p.value === gitProxy) return;
    gitProxy = p.value;
    const kept = gitProxy ? store.set('gitProxy', gitProxy) : store.remove('gitProxy');
    if (worker) worker.postMessage({ type: 'gitProxy', gitProxy });
    settingsChanged();
    note(gitProxy ? 'Git requests now go through the proxy at ' + p.origin + '.'
                  : 'Git requests now go to the repositories themselves, without a proxy.');
    if (!kept) showNotice('This browser cannot keep the setting: it holds for this session only.', null, null, 8000);
  }
  $('settings').addEventListener('click', openSettings);
  proxyInput.addEventListener('input', showProxyState);
  $('settings-form').addEventListener('submit', e => { e.preventDefault(); saveSettings(); });
  $('git-proxy-clear').addEventListener('click', () => { proxyInput.value = ''; showProxyState(); proxyInput.focus(); });
  $('settings-cancel').addEventListener('click', () => dialog.close());
  dialog.addEventListener('close', () => refocus());
  // a click on the backdrop, outside the form, cancels
  dialog.addEventListener('click', e => { if (e.target === dialog) dialog.close(); });
  // the Settings of another tab of the Console
  addEventListener('storage', e => {
    if (e.key !== 'pharo-wasm.gitProxy' && e.key !== null) return;
    const now = parseGitProxy(store.get('gitProxy', '')).value || '';
    if (now === gitProxy) return;
    gitProxy = now;
    if (worker) worker.postMessage({ type: 'gitProxy', gitProxy });
    settingsChanged();
  });
  // (the notebook's VM gets it too)
  function settingsChanged() {
    dispatchEvent(new CustomEvent('pharo:settings', { detail: { gitProxy } }));
  }

  // ---- input and history

  let history = [];
  try { history = JSON.parse(store.get('history', '[]')); } catch (e) { history = []; }
  if (!Array.isArray(history)) history = [];
  let histIdx = history.length, draft = '';

  function autosize() {
    line.style.height = 'auto';
    line.style.height = line.scrollHeight + 'px';
  }

  function submit() {
    if (!worker || !alive || !ready) return;
    const text = line.value;
    if (text.trim() && history[history.length - 1] !== text) {
      history.push(text);
      if (history.length > HISTORY_MAX) history = history.slice(-HISTORY_MAX);
      store.set('history', JSON.stringify(history));
    }
    histIdx = history.length;
    draft = '';
    if (text.trim()) evaluated = true;
    // a chunk is evaluated at its LF: the line breaks within go as CRs
    worker.postMessage({ type: 'input', text: text.replace(/\r\n?|\n/g, '\r') + '\n' });
    line.value = '';
    autosize();
    // evaluating until the worker reports otherwise, which may be never: a
    // long primitive does not yield (and then Stop needs the watchdog)
    echo(text);
  }

  function recall(dir) {
    if (!history.length) return;
    if (histIdx === history.length) draft = line.value;
    histIdx = Math.max(0, Math.min(history.length, histIdx + dir));
    line.value = histIdx === history.length ? draft : history[histIdx];
    autosize();
    const end = line.value.length;
    line.setSelectionRange(end, end);
  }

  line.addEventListener('input', autosize);
  addEventListener('resize', autosize);
  // A key of an input method: its composition, or (WebKit) the key that
  // ended it, which comes after compositionend
  const composing = e => e.isComposing || e.keyCode === 229;
  line.addEventListener('keydown', e => {
    const v = line.value, a = line.selectionStart, b = line.selectionEnd;
    if (composing(e)) return;
    if (e.key === 'Enter' && !e.shiftKey && !e.altKey) {
      e.preventDefault();
      submit();
    } else if (e.key === 'Tab' && !e.shiftKey && !e.ctrlKey && !e.altKey && v) {
      e.preventDefault();                 // on an empty line Tab moves focus as usual
      line.setRangeText('\t', a, b, 'end');
      autosize();
    } else if (e.key === 'ArrowUp' && !e.shiftKey && !e.altKey && a === b && v.lastIndexOf('\n', a - 1) < 0) {
      e.preventDefault();
      recall(-1);
    } else if (e.key === 'ArrowDown' && !e.shiftKey && !e.altKey && a === b && v.indexOf('\n', a) < 0) {
      e.preventDefault();
      recall(1);
    } else if ((e.ctrlKey || e.metaKey) && !e.shiftKey && e.key.toLowerCase() === 'd' && !v) {
      e.preventDefault();
      sendEof();
    }
  });

  document.addEventListener('keydown', e => {
    if (composing(e)) return;           // Escape cancels the composition
    if (dialog.open) return;            // its keys: Escape closes it
    if (!consoleVisible()) return;      // the notebook's (notebook.js)
    const k = e.key.toLowerCase();
    if (e.key === 'Escape') { e.preventDefault(); stop(); }
    else if (e.ctrlKey && !e.shiftKey && !e.altKey && !e.metaKey && k === 'c') {
      // Ctrl-C copies when something is selected, else it stops
      const sel = getSelection();
      if ((sel && !sel.isCollapsed) || line.selectionStart !== line.selectionEnd) return;
      e.preventDefault();
      stop();
    } else if (e.ctrlKey && !e.shiftKey && !e.altKey && k === 'l') {
      e.preventDefault();
      clearTerm();
    }
  });

  // Clicking the terminal (without selecting) focuses the input line.
  term.addEventListener('mouseup', () => {
    const sel = getSelection();
    if ((!sel || sel.isCollapsed) && !line.disabled) line.focus({ preventScroll: true });
  });

  const refocus = () => { if (!touch && !line.disabled && consoleVisible()) line.focus(); };
  $('entry').addEventListener('submit', e => { e.preventDefault(); submit(); line.focus(); });
  $('stop').addEventListener('click', () => { stop(); refocus(); });
  $('restart').addEventListener('click', restart);
  $('save').addEventListener('click', () => { save(); refocus(); });
  $('download').addEventListener('click', download);
  $('open').addEventListener('click', chooseFile);
  $('open-file').addEventListener('change', e => {
    const files = [...e.target.files];
    e.target.value = '';
    openFiles(files);
  });
  $('reset').addEventListener('click', resetSaved);
  $('clear').addEventListener('click', () => { clearTerm(); refocus(); });
  $('theme').addEventListener('click', cycleTheme);
  $('notice-action').addEventListener('click', () => { const f = noticeAction; hideNotice(); if (f) f(); });
  $('notice-alt').addEventListener('click', () => { const f = noticeAlt; hideNotice(); if (f) f(); });
  $('notice-close').addEventListener('click', hideNotice);
  $('hist-up').addEventListener('click', () => { recall(-1); line.focus(); });
  $('hist-down').addEventListener('click', () => { recall(1); line.focus(); });

  // The worker stores the .changes of the saved image soon after an
  // evaluation; when the page goes away, it is asked to store what is left,
  // which it may not live to do.  A hidden page acks the output it has.
  const flush = () => { if (worker) worker.postMessage({ type: 'flush' }); };
  addEventListener('pagehide', flush);
  document.addEventListener('visibilitychange', () => {
    if (document.visibilityState !== 'hidden') return;
    ack();
    flush();
  });

  // ---- tabs: the Console and the Notebook (notebook.js)

  const tabs = [$('tab-console'), $('tab-notebook')];
  const tabName = t => t.id.replace(/^tab-/, '');
  const activeTab = () => consoleVisible() ? 'console' : 'notebook';
  // focus: from the keys of the tablist, which keep the focus there
  function selectTab(tab, focus, quiet) {
    for (const t of tabs) {
      const on = t === tab;
      t.setAttribute('aria-selected', String(on));
      t.tabIndex = on ? 0 : -1;
      $(t.getAttribute('aria-controls')).hidden = !on;
    }
    const nb = tab.id === 'tab-notebook';
    $('toolbar').hidden = nb;
    $('nb-toolbar').hidden = !nb;
    statusPill.hidden = nb;             // the notebook has its own
    store.set('tab', tabName(tab));
    if (focus) tab.focus();
    if (!nb) {
      render();
      term.scrollTop = term.scrollHeight;
      if (!focus) refocus();
    }
    if (!quiet) dispatchEvent(new CustomEvent('pharo:tab', { detail: { tab: tabName(tab) } }));
  }
  for (const t of tabs) {
    t.addEventListener('click', () => selectTab(t));
    t.addEventListener('keydown', e => {
      const i = tabs.indexOf(t);
      let j = null;
      if (e.key === 'ArrowRight') j = (i + 1) % tabs.length;
      else if (e.key === 'ArrowLeft') j = (i + tabs.length - 1) % tabs.length;
      else if (e.key === 'Home') j = 0;
      else if (e.key === 'End') j = tabs.length - 1;
      if (j !== null) { e.preventDefault(); selectTab(tabs[j], true); }
    });
  }
  // The tab of the last visit.  Its event goes once the scripts after this
  // one (notebook.js) listen.
  const lastTab = tabs.find(t => tabName(t) === store.get('tab', 'console'));
  if (lastTab && lastTab !== tabs[0]) selectTab(lastTab, false, true);
  document.addEventListener('DOMContentLoaded', () => {
    dispatchEvent(new CustomEvent('pharo:tab', { detail: { tab: activeTab() } }));
  });

  // ---- what the notebook (notebook.js) gets of the page

  window.PharoPage = Object.freeze({
    Q, BUILD, store, touch, HTTP_HINT, countLines, siteKey,
    // null, or why no VM can run in this page (then none is started)
    unavailable: () => unavailable || (noMemory64 ? NO_MEMORY64 : null),
    wasmModule,                         // Promise<WebAssembly.Module>, compiled once
    workerUrl: 'vm-worker.js' + Q,
    gitProxy: () => gitProxy,           // '' when none
    gitHttp: () => gitHttp,             // the VM has libgit2's smart-HTTP transport (ready)
    openSettings, cycleTheme,
    themeLabel: () => THEMES[theme],    // System, Light or Dark
    activeTab,                          // 'console' or 'notebook'
  });

  // ---- start

  if (location.protocol === 'file:')
    unavailable = HTTP_HINT + '\nBrowsers do not run workers or fetch .wasm files from file:// URLs.';
  else if (typeof WebAssembly !== 'object' || typeof Worker !== 'function')
    unavailable = 'This browser lacks WebAssembly or Web Workers.';
  else if (typeof DecompressionStream !== 'function')
    unavailable = 'This browser cannot inflate the files of Pharo (it has no DecompressionStream): use a newer one.';
  if (unavailable) {
    term.textContent = '';
    fatal(unavailable);
  } else {
    spawn();
  }
})();
