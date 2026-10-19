// world.js - the world page (world.html) of the Pharo VM for WebAssembly
//
// The page side of the Pharo world: owns the VM's worker (vm-worker.js, see
// there for the message protocol) in 'world' mode, whose display
// (display-worker.js) draws the world into the canvas of this page, which is
// transferred to it as an OffscreenCanvas.  The page sends the display the
// size of the canvas and its input, as the event records of the image's
// OSWebDriver (packaging/emscripten/st/OSWindow-Web), and shows what the
// display asks for.  No dependencies but keymap.js, open-image.js,
// vm-storage.js, catalog.js, git-proxy.js and images.js.
//
// - pharo-web.wasm is compiled once here and handed to every worker, so a
//   restart only instantiates it.  A canvas can be transferred only once:
//   every worker gets a new one.  A generation number per worker drops the
//   messages of a worker that was replaced.
// - The canvas has one pixel per CSS pixel.  Its size goes to the display at
//   once, before the image boots, then 100 ms after it last changed.
// - Pointer: buttons and masks numbered as SDL does (keymap.js), captured
//   while held; moves are sent once per animation frame, the last one, and
//   before any other event.  The context menu is the image's.  The wheel
//   sends notches of 40 px, positive away from the user.
// - Keyboard (keymap.js): a key down, the text it types, a key up.  The keys
//   the world handles are kept from the browser, but copy, cut and paste:
//   Ctrl/Cmd+V waits for the paste event, whose text goes first.  When the
//   page loses the focus, the keys and buttons held are released.  The
//   browser keeps some keys for itself (Ctrl+W, the second key of Pharo's
//   Ctrl+O, Ctrl+W, closes the tab): while the world took input that is not
//   saved, the browser asks before the page goes (beforeunload).  F6 (or
//   Shift+F6), which the image does not use, moves the keyboard from the
//   world to the toolbar, and Tab from there goes back into the world.
// - Stop interrupts the image, which opens a debugger on the busy process;
//   when the worker then stays silent for 3 s (a long primitive that never
//   yields), it is replaced.  Its answers to the Stop and to the page's
//   requests do not count: they come between two slices, and the next one
//   may be the long one.
// - The status pill says Busy when the VM has not slept for a second, or
//   when the worker, silent for a second, does not answer a request within
//   another one: an idle world says nothing, and a VM in a long primitive
//   after an idle slice could not.
// - The image: the URL says which (?slot=<slot> of the images saved in this
//   browser, ?fresh=<id> of the site's catalog; nothing else of the URL is
//   read), else the worker boots the image saved or booted last, else the
//   default one of the catalog.  On a first visit (nothing saved) to a site
//   of several images, no VM starts: the Images dialog opens as the
//   launcher (images.js), and the user chooses which.  Once the VM is
//   ready, and after each save, the URL says the image that runs
//   (history.replaceState), and so does the link to the Console.  An image
//   opened (Open) is none of them until the worker kept it in a slot: the
//   URL then names no image, and Images shows no slot running; when it
//   does not start, Restart boots the image of before again.
// - Save asks the image to save itself (a save request record); the worker
//   keeps the saved image in this browser: in the slot it booted from, or
//   in a new one for an image started fresh or opened, named after it.
//   Save as names a new slot first (the worker's saveAs), which the world
//   then saves into, and is bound to from then on.  Download gives the
//   image and its .changes.  A save keeps the files of /pharo with the
//   image too (Iceberg's clones: the worker's tree).
// - Images lists the images of the site, to start fresh, and those saved
//   in this browser, to start, rename or delete (the one that runs: its VM
//   starts again); it starts one, after a question when the world has input
//   that is not saved.
// - Open (or files dropped on the page) starts an image of your own, as in
//   the Console: open-image.js chooses, unpacks and checks the files while
//   the world goes on, the notice saying how far it got; it asks first when
//   the world has input that is not saved.  The image becomes a new slot
//   once it has started, and never replaces another one.
// - An image that cannot open the world (it lacks OSWindow-Web: a stock
//   image, opened here or in the Console; or it has an older version of
//   it, saved by an earlier build of the site) is prepared for it: the
//   worker (init.prepare) files in st/web-bootstrap.st and saves it, as the
//   Console's "Prepare for the world" does, and says "prepared", with the
//   slot that holds it now; the page then boots the world from that slot.
//   The notice names the fonts that the preparation set up (ready.fonts,
//   from the manifest: FreeType, or bitmap fonts).
// - The display's title, cursor (RGBA, as a CSS cursor), clipboard
//   (navigator.clipboard.writeText) and focus.  When the browser refuses to
//   write the clipboard, the system clipboard still has older text, which
//   the next paste would bring over the image's own copy: the page then
//   keeps the text of its pastes from the image until it lost the focus to
//   another window, or a copy of the page changed the clipboard.
// - A world that did not paint after START_MS, or a VM that ended before
//   it painted, may be an image that cannot open the world: the notice
//   points to the Console, on that image, which can run it, or delete it.
// - The status pill is no live region: the one next to it says how the VM
//   goes (loading, starting, running, ended), not how far the loading got.
// - The CORS proxy of git's HTTP requests (git-proxy.js): the site's
//   default (manifest.gitProxyDefault), or the one, or none, that the user
//   chose in Settings, here or in the Console, which this browser keeps.
//   It is read at each start of a worker and given in init.gitProxy (null
//   for the default), and given to the running VM when Settings or another
//   page changes it (a storage event).  Like the Console, the page never
//   takes it from its URL.  The worker says when git made its first request
//   (git {event: 'request'}): through the site's default, the page tells
//   once, per proxy, that the public proxy sees what is cloned; and when one
//   failed, why, with a link to Settings.
//
// window.PharoWorld tells tests and the curious how it goes: stats (the
// workers started, the frames of this one, when they came and the size of
// the first, the times from a key to the next frame), state (that of the
// status pill), unsaved (whether closing the page would ask), choice (the
// image that runs: {slot}, {fresh} or {}), images (the Images dialog),
// readFile(path), a file of the VM, and fs(op, path, fields), the worker's
// other "fs" requests.

(function () {
  'use strict';

  const RUNNING = 0, WAITING = 1, BUSY = 2, SLEEPING = 4;
  // The record types of OSWebDriver
  const MOVE = 1, PRESS = 2, RELEASE = 3, WHEEL = 4, KEY_DOWN = 5, KEY_UP = 6, TEXT = 7, FOCUS = 9, SAVE_REQUEST = 11;
  const WATCHDOG_MS = 3000;
  const RESIZE_MS = 100;                // a resize is sent when the size stayed so long
  const PASTE_MS = 200;                 // Ctrl+V waits so long for its paste event
  const BUSY_MS = 1000;                 // busy, when the VM has not slept for so long
  const START_MS = 30000;               // the world paints so long after ready, or did not open
  const REVOKE_MS = 60000;              // a download has long begun then: its Blob may go
  const NOTCH = 40;                     // wheel pixels per notch
  const LINE = NOTCH / 3, PAGE = 800;   // and per line and page
  const BUTTONS = [[1, 1], [2, 2], [4, 3]];   // SDL masks and their button numbers

  const BUILD = (document.querySelector('meta[name="pharo-build"]') || {}).content || '';
  const Q = BUILD && BUILD.indexOf('@') < 0 ? '?v=' + encodeURIComponent(BUILD) : '';
  const $ = id => document.getElementById(id);
  const stage = $('stage'), sink = $('sink'), statusPill = $('status'), statusText = $('status-text');
  const Keymap = window.PharoKeymap;
  const OS = Keymap.platform();
  let canvas = $('world');

  // The theme of the Console page, when it chose one
  try {
    const theme = localStorage.getItem('pharo-wasm.theme');
    if (theme === 'light' || theme === 'dark') document.documentElement.dataset.theme = theme;
  } catch (e) { /* no storage: follow the system */ }

  const GitProxy = window.PharoGitProxy, Catalog = window.PharoCatalog, Storage = window.PharoStorage;
  // The proxy of git's HTTP requests, as another page changes it (Settings
  // of the Console, or of another world)
  GitProxy.onChange(value => { if (worker) worker.postMessage({ type: 'gitProxy', gitProxy: value }); });

  // ---- status, overlay and notice

  let worker = null, gen = 0, ready = false, alive = false, framed = false, state = null;
  let lastMsgAt = 0, idleAt = 0, watchdog = 0, saving = false, loadedText = 'Loading', startTimer = 0;
  let unavailable = null;               // why the page cannot run the world at all
  let heardAt = 0, pingAt = 0;          // when the worker last said anything, and was asked
  let restarted = '';                   // why the worker was replaced, said until the next one is ready
  let upload = null;                    // {name, image, changes, sources} to boot until the worker keeps it
  let beforeUpload = {};                // the choice that ran before it, which Restart boots if it was not kept
  let kept = false;                     // the worker kept the upload it booted (or failed to)
  let source = null, persisted = false; // where the image came from, and whether one is saved (ready)
  let preparing = false;                // the worker prepares the image for the world (ready.preparing)
  let fonts = null;                     // the fonts that a preparation sets up (ready.fonts)
  let opening = false;                  // Open unpacks files
  // The image of the page's VM: {slot}, {fresh: id} or {} (the worker's
  // choice), the name of its slot, and its catalog entry (ready.imageId)
  let current = {}, currentName = '', currentEntry = null;
  let manifest = null;                  // the site's (null until fetched, or when it could not be)
  let images = null;                    // the Images dialog (images.js)
  let choosing = false;                 // the launcher shows, and no VM runs
  // the images saved in this browser (vm-storage.js), or null when it
  // keeps none here
  let store = null;
  try { store = Storage.open(); } catch (e) { store = null; }
  const reqs = new Map();
  let reqId = 0;
  // workers started; frames of this one, when it started and they came, and
  // the [width, height] of the first; the last times from a key to the next
  // frame
  const stats = { starts: 0, startedAt: 0, frames: 0, firstFrameAt: 0, lastFrameAt: 0, firstFrameSize: null,
                  keyToFrame: [] };
  let keyAt = 0;                        // when a key went that no frame showed yet

  function setStatus(key, text) {
    statusPill.dataset.state = key;
    if (statusText.textContent !== text) statusText.textContent = text;
    if (key === 'loading') say('Loading Pharo');
    else if (key === 'starting') say('Starting Pharo');
    else if (key === 'preparing') say('Preparing the image for the world');
    else if (key === 'running' && said === 'Starting Pharo') say('The Pharo world is running');
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
    if (preparing) return setStatus('preparing', 'Preparing');
    if (!framed) return setStatus('starting', 'Starting');
    if (saving) return setStatus('saving', 'Saving');
    const now = performance.now();
    if (state === BUSY && now - idleAt > BUSY_MS) return setStatus('busy', 'Busy');
    if (pingAt && now - pingAt > BUSY_MS) return setStatus('busy', 'Busy');
    return setStatus('running', 'Running');
  }
  // The worker answers between two slices: ask it something when it was
  // silent for a while (the answer, a list of /pharo, is dropped)
  function ping() {
    if (!worker || !alive || !ready || pingAt || performance.now() - heardAt < BUSY_MS) return;
    const g = gen;
    pingAt = performance.now();
    request({ type: 'fs', op: 'listDir', path: '/pharo' }).catch(() => {}).then(() => {
      if (g !== gen) return;
      pingAt = 0;
      updateStatus();
    });
  }
  setInterval(() => { ping(); updateStatus(); }, BUSY_MS / 4);
  function updateControls() {
    const on = !!worker && alive && ready && !preparing;
    $('stop').disabled = !on;
    $('save').disabled = $('save-as').disabled = !(on && framed);
    $('download').disabled = !(worker && ready);
    $('open').disabled = !!unavailable || opening;
    $('images').disabled = !!unavailable;
  }

  // the card over the canvas: a text, and how far the loading is (0..1),
  // or null (going on), or false (not loading)
  function showOverlay(text, fraction) {
    const card = $('loading');
    $('overlay').hidden = false;
    $('overlay-text').textContent = text;
    $('track').hidden = fraction === false;
    $('track').classList.toggle('indefinite', fraction == null);
    $('fill').style.width = fraction == null ? '' : (100 * fraction).toFixed(1) + '%';
    if (fraction === false) {
      for (const a of ['role', 'aria-label', 'aria-valuetext', 'aria-valuenow']) card.removeAttribute(a);
    } else {
      card.setAttribute('role', 'progressbar');
      card.setAttribute('aria-label', 'Loading Pharo');
      card.setAttribute('aria-valuetext', text);
      if (fraction == null) card.removeAttribute('aria-valuenow');
      else card.setAttribute('aria-valuenow', String(Math.round(100 * fraction)));
    }
  }
  const hideOverlay = () => { $('overlay').hidden = true; };

  // The notice over the canvas: text, and a button doing action, or a link
  // to the page action, and a second link alt {label, href}, if any
  let noticeAction = null, noticeTimer = 0;
  function showNotice(text, label, action, ms, alt) {
    clearTimeout(noticeTimer);
    $('notice-text').textContent = text;
    const link = typeof action === 'string';
    $('notice-action').hidden = !label || link;
    $('notice-action').textContent = label && !link ? label : '';
    $('notice-link').hidden = !label || !link;
    $('notice-link').textContent = label && link ? label : '';
    if (link) $('notice-link').href = action;
    $('notice-alt').hidden = !alt;
    $('notice-alt').textContent = alt ? alt.label : '';
    if (alt) $('notice-alt').href = alt.href;
    noticeAction = link ? null : action || null;
    $('notice').hidden = false;
    if (ms) noticeTimer = setTimeout(hideNotice, ms);
  }
  function hideNotice() {
    clearTimeout(noticeTimer);
    $('notice').hidden = true;
    noticeAction = null;
  }
  // The Console, on the image of this page
  const query = c => c && c.slot ? '?slot=' + encodeURIComponent(c.slot) : c && c.fresh ? '?fresh=' + encodeURIComponent(c.fresh) : '';
  const consoleHref = () => 'index.html' + query(current);
  const CONSOLE = { label: 'Open the Console', get href() { return consoleHref(); } };
  // The image chosen is the one that the URL and the link to the Console say
  function setChoice(choice) {
    current = choice || {};
    $('console').href = consoleHref();
    try { window.history.replaceState(window.history.state, '', location.pathname + query(current) + location.hash); }
    catch (e) { /* (a sandboxed page) */ }
  }

  const mb = n => (n / 1048576).toFixed(1) + ' MB';

  // ---- the VM's worker

  let wasmP = null;
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

  const HTTP_HINT = 'This page must be served over HTTP, for example with "make wasm-serve".';

  function kill() {
    gen++;
    clearTimeout(watchdog);
    clearTimeout(startTimer);
    flushPaste(true);
    if (worker) worker.terminate();
    worker = null;
    alive = ready = framed = saving = preparing = kept = false;
    state = null;
    pingAt = 0;
    for (const r of reqs.values()) r.reject(new Error('the VM was restarted'));
    reqs.clear();
    held.clear();
    pointer.buttons = 0;
    unsaved = touched = inputWhileSaving = false;
    staleClipboard = false;
    sink.tabIndex = -1;
  }

  // The page cannot run the world: say why, and offer the action, if any
  function fatal(msg, label, action) {
    kill();
    restarted = '';
    setStatus('error', 'Unavailable');
    showOverlay(msg.split('\n')[0], false);
    updateControls();
    showNotice(msg.replace(/\n/g, ' '), label, action);
  }

  // A canvas can be transferred to a worker once: every worker gets a new
  // one.  It can take the focus, which it hands to the sink (see lostFocus)
  function freshCanvas() {
    const c = document.createElement('canvas');
    c.id = 'world';
    c.setAttribute('aria-label', canvas.getAttribute('aria-label'));
    c.style.cursor = 'default';
    c.tabIndex = -1;
    c.addEventListener('focus', focusSink);
    c.addEventListener('blur', lostFocus);
    canvas.replaceWith(c);
    canvas = c;
    return c;
  }

  // Start a worker on the image choice ({slot}, {fresh: id} or {}: the
  // worker's choice), or, without one, on the image of the page again (a
  // restart); an upload goes first
  async function spawn(choice) {
    kill();
    if (choice && typeof choice === 'object' && !(choice instanceof Event)) setChoice(choice);
    if (choosing) { choosing = false; if (images) images.close(); }
    const g = gen;
    alive = true;
    loadedText = 'Loading';
    stats.starts++;
    stats.startedAt = performance.now();
    stats.frames = stats.firstFrameAt = stats.lastFrameAt = 0;
    stats.firstFrameSize = null;
    if (restarted) showNotice(restarted, null, null); else hideNotice();
    showOverlay('Loading Pharo…', null);
    updateControls();
    updateStatus();
    let mod;
    try {
      mod = await (wasmP || (wasmP = compileWasm('pharo-web.wasm' + Q)));
    } catch (e) {
      wasmP = null;
      if (g === gen) fatal('Could not load pharo-web.wasm: ' + ((e && e.message) || e) + '\n' + HTTP_HINT, 'Restart', spawn);
      return;
    }
    if (g !== gen) return;
    const { width, height } = stageSize();
    const c = freshCanvas();
    c.width = Math.max(1, Math.round(width * ratio()));
    c.height = Math.max(1, Math.round(height * ratio()));
    let offscreen, w;
    try {
      offscreen = c.transferControlToOffscreen();
      w = new Worker('vm-worker.js' + Q);
    } catch (e) {
      fatal('Could not start the VM worker: ' + ((e && e.message) || e) + '\n' + HTTP_HINT, 'Restart', spawn);
      return;
    }
    worker = w;
    w.onmessage = e => { if (g === gen) onWorker(e.data); };
    w.onerror = e => {
      if (g !== gen) return;
      e.preventDefault();
      console.warn('vm-worker.js: ' + (e.message || 'failed to load'));
      fatal('The VM worker failed: ' + ((e && e.message) || 'could not load vm-worker.js') + '\n' + HTTP_HINT, 'Restart', spawn);
    };
    w.onmessageerror = () => { if (g === gen) fatal('A message of the VM worker could not be decoded.', 'Restart', spawn); };
    w.postMessage({
      type: 'init',
      wasmModule: mod,
      manifestUrl: 'manifest.json' + Q,
      build: BUILD,
      mode: 'world',
      persist: true,
      upload: upload || undefined,
      slot: current.slot,
      fresh: current.fresh,
      name: upload ? upload.name : undefined,
      prepare: true,
      display: { canvas: offscreen, width, height },
      gitProxy: GitProxy.initValue(),
    }, [offscreen]);
    // queued by the worker until it is ready, so before the image boots,
    // which then opens the world at the size of the canvas
    sentSize = null;
    sendSize();
  }

  // The VM ended; the worker stays, for Download.  Before the world
  // painted, the image may be one that cannot open it: the Console can run
  // it, or reset it.  An upload that did not start is not kept, and Restart
  // boots the saved image.
  function ended(status, text, notice) {
    clearTimeout(watchdog);
    clearTimeout(startTimer);
    if (!framed && !kept && source === 'upload') {
      upload = null;
      setChoice(beforeUpload);
      notice = 'The opened image did not start; it was not kept. ' + notice;
    } else if (preparing) {
      notice = 'The image could not be prepared for the world: ' + notice;
    }
    alive = false;
    restarted = '';
    saving = preparing = false;
    releaseAll(false);
    sink.tabIndex = -1;
    setStatus(status, text);
    if (!framed) showOverlay(notice, false);
    updateControls();
    showNotice(notice, 'Restart', spawn, 0, framed ? null : CONSOLE);
  }

  function respawn() {
    if (!framed && !kept && source === 'upload') {   // it hung before it started
      upload = null;
      setChoice(beforeUpload);
    }
    restarted = 'VM restarted (unsaved changes lost).';
    spawn();
  }

  // The worker prepared the image for the world (m.error absent), and the
  // world boots from the image saved in this browser; or it could not: the
  // VM still runs the REPL, which Download can save from
  function onPrepared(m) {
    preparing = false;
    if (!m.error) {
      upload = null;
      restarted = 'The image is prepared for the world (OSWindow-Web and ' +
        (fonts === 'freetype' ? 'FreeType' : 'bitmap') + ' fonts), and saved in this browser' +
        (currentName ? " as '" + currentName + "'." : '.');
      spawn(m.slot ? { slot: m.slot } : undefined);
      return;
    }
    console.warn('pharo: the image could not be prepared for the world: ' + m.error);
    alive = false;
    setStatus('error', 'Not prepared');
    showOverlay('This image could not be prepared for the world.', false);
    updateControls();
    const why = String(m.error).split('\n')[0].replace(/\.$/, '');
    if (m.saved) showNotice('This image could not be prepared for the world: ' + why + '.', 'Download', download, 0, CONSOLE);
    else showNotice('This image could not be prepared for the world: ' + why + '.', 'Restart', spawn, 0, CONSOLE);
  }

  // "Pharo 13.1", or '' when unknown
  const pharoLabel = pharo => pharo ? Catalog.labelOf(pharo) : '';

  function onReady(m) {
    ready = true;
    source = m.source;
    persisted = m.persisted;
    fonts = m.fonts;
    currentEntry = (manifest && m.imageId && Catalog.find(manifest, m.imageId)) || null;
    // the image that runs: its slot, or the catalog entry it is a fresh copy
    // of (an upload is neither until it is kept)
    if (m.slot) {
      // (a slot without a name of its own is called as Images calls it)
      currentName = m.slot.name || (m.slot.slot === 'main' && !m.slot.origin ? 'Saved image'
        : m.pharo ? Catalog.labelOf(m.pharo) : 'Saved image');
      setChoice({ slot: m.slot.slot });
    } else if (m.source !== 'upload') {
      currentName = '';
      setChoice(m.imageId ? { fresh: m.imageId } : {});
    }
    if (!m.world) {
      fatal('This build has no world image: build it with WASM_WORLD=ON and a host Pharo (WASM_HOST_PHARO).',
            'Open the Console', consoleHref());
      return;
    }
    if (!m.prepared && !m.preparing) {
      fatal('This image cannot open the Pharo world: it lacks OSWindow-Web, or has an older version of it.',
            'Open the Console', consoleHref());
      return;
    }
    const notes = [restarted];
    restarted = '';
    const label = (currentEntry && currentEntry.label) || pharoLabel(m.pharo) || 'Pharo';
    if (m.slotMissing) notes.push('That saved image is not in this browser any more: started a fresh ' + label + '.');
    else if (m.freshMissing) notes.push("This site has no image '" + m.freshMissing + "': started " + label + '.');
    if (m.source === 'saved')
      notes.push('Started the image saved in this browser on ' + new Date(m.savedAt).toLocaleString() + ': ' +
                 (currentName ? "'" + currentName + "'" : 'it') + (m.pharo ? ' (' + pharoLabel(m.pharo) + ').' : '.'));
    else if (m.source === 'upload')
      notes.push('Started the opened image, ' + (upload ? upload.name : m.image) + '.');
    else if (!m.slotMissing && !m.freshMissing && Catalog.images(manifest).length > 1)
      notes.push('Started a fresh ' + label + '. Save keeps it in this browser.');
    if (m.sourcesMissing)
      notes.push("This image's .sources (" + m.sourcesMissing + ') is not on this site any more: ' +
                 'the source of system methods shows decompiled.');
    if (m.storageError) notes.push('Note: ' + m.storageError + '.');
    const note = notes.filter(Boolean).join(' ');
    if (m.preparing) {
      // a stock image: OSWindow-Web is filed in, and the image saved
      preparing = true;
      showOverlay('Preparing the image for the world: filing in OSWindow-Web, then saving it…', null);
      if (note) showNotice(note, null, null, m.storageError ? 10000 : 6000);
      updateControls();
      updateStatus();
      return;
    }
    if (note) showNotice(note, null, null, m.storageError ? 10000 : 6000);
    showOverlay('Starting Pharo…', null);
    const g = gen;
    startTimer = setTimeout(() => {
      if (g !== gen || framed || !alive) return;
      notStarted = true;
      showNotice('The world has not opened yet: this image may not open it. The Console can run it, ' +
                 'or delete it.', 'Open the Console', consoleHref(), 0);
    }, START_MS);
    updateControls();
    updateStatus();
  }
  let notStarted = false;               // the notice of startTimer is shown

  function onFrame(m) {
    const t = performance.now();
    stats.frames++;
    stats.lastFrameAt = t;
    if (keyAt) {
      stats.keyToFrame.push(t - keyAt);
      if (stats.keyToFrame.length > 200) stats.keyToFrame.shift();
      keyAt = 0;
    }
    if (framed) return;
    framed = true;
    clearTimeout(startTimer);
    if (notStarted) hideNotice();       // it did, after all
    notStarted = false;
    sink.tabIndex = 0;
    stats.firstFrameAt = t;
    stats.firstFrameSize = [m.width, m.height];
    hideOverlay();
    // this image has not been told yet that it has the focus
    focused = false;
    focusSink();
    if (document.activeElement === sink) gainedFocus();
    updateControls();
    updateStatus();
  }

  function onDisplay(m) {
    switch (m.kind) {
    case 'frame': onFrame(m); break;
    case 'displayOpen': case 'title': setTitle(m.title); break;
    case 'cursor': setCursor(m); break;
    case 'clipboardSet': writeClipboard(m.text); break;
    case 'focus': focusSink(); break;
    }
  }

  // What the image writes on stdout and stderr goes to the browser's console
  function log(fd, text) {
    const t = text.replace(/\n$/, '');
    if (fd === 2) console.warn('[pharo] ' + t); else console.log('[pharo] ' + t);
  }

  function onWorker(m) {
    heardAt = performance.now();
    // a slice ran: the sign of life that Stop's watchdog waits for
    if (m.type !== 'interrupted' && m.type !== 'fs-result' && m.type !== 'file' && m.type !== 'error' &&
        m.type !== 'reset')
      lastMsgAt = heardAt;
    switch (m.type) {
    case 'progress': {
      const fraction = m.total ? m.loaded / m.total : 1;
      loadedText = m.phase === 'boot' ? 'Starting' : 'Loading ' + Math.floor(100 * fraction) + '%';
      if (m.phase === 'boot') showOverlay('Starting Pharo…', null);
      else showOverlay((m.phase === 'restore' ? 'Restoring the saved image… ' : 'Downloading Pharo… ') +
                       Math.floor(100 * fraction) + '%', fraction);
      updateStatus();
      break;
    }
    case 'ready':
      onReady(m);
      break;
    case 'output':
      log(m.fd, m.text);
      worker.postMessage({ type: 'ack', chars: m.text.length });
      break;
    case 'state':
      state = m.state;
      if (state === SLEEPING || state === WAITING || state === RUNNING) idleAt = lastMsgAt;
      updateStatus();
      break;
    case 'interrupted':
      if (!m.registered && framed)
        showNotice('This image registered nothing for Stop to interrupt.', 'Restart', spawn, 10000);
      break;
    case 'saved':
      // the slot that holds the image now, which the page then names
      if (!m.error && m.slot && m.slot.slot) {
        currentName = m.slot.name || currentName;
        setChoice({ slot: m.slot.slot });
      }
      if (m.upload) {                   // the upload booted, and is kept
        kept = true;
        if (m.error) showNotice('The opened image runs, but ' + m.error + '.', 'Download', download);
        else {
          upload = null;
          persisted = true;
          if (!preparing)
            showNotice('The opened image is now kept in this browser' + (currentName ? " as '" + currentName + "'." : '.'),
                       'Rename', renameCurrent, 10000);
        }
        break;
      }
      if (preparing) break;             // "prepared" says how it went
      saving = false;
      if (m.error) showNotice('Saved, but ' + m.error + '.', 'Download', download);
      else {
        unsaved = inputWhileSaving;
        persisted = true;
        const text = 'The image is saved in this browser' + (currentName ? " as '" + currentName + "'" : '') +
          ' (' + mb(m.bytes) + ').' + (m.treeError ? ' Note: ' + m.treeError + '.' : '');
        if (m.created) showNotice(text, 'Rename', renameCurrent, m.treeError ? 0 : 10000);
        else showNotice(text, null, null, m.treeError ? 0 : 6000);
      }
      inputWhileSaving = false;
      updateStatus();
      break;
    case 'prepared':
      onPrepared(m);
      break;
    case 'superseded':
      // what this VM did since it started is kept nowhere now
      unsaved = touched;
      showNotice('Another tab has replaced the image saved in this browser' + (currentName ? " ('" + currentName + "')" : '') +
                 '. Save to keep this session instead.', 'Save', save);
      break;
    case 'git':
      onGit(m);
      break;
    case 'exit':
      ended('exited', 'Exited (' + m.code + ')', 'Pharo quit with exit code ' + m.code + '.');
      break;
    case 'crash':
      console.warn('pharo: ' + m.message + (m.stacks ? '\n' + m.stacks : '') + (m.stack ? '\n' + m.stack : ''));
      ended('crashed', 'Crashed', 'The VM crashed: ' + String(m.message).split('\n')[0]);
      break;
    case 'display':
      onDisplay(m);
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

  function request(msg) {
    return new Promise((resolve, reject) => {
      if (!worker) { reject(new Error('the VM is not running')); return; }
      const id = ++reqId;
      reqs.set(id, { resolve, reject, files: [] });
      worker.postMessage(Object.assign({ id }, msg));
    });
  }

  // ---- what the display asks for

  function setTitle(title) {
    let t = String(title || '');
    if (t.startsWith('/')) t = t.slice(t.lastIndexOf('/') + 1);   // the image's path
    document.title = t ? 'Pharo - ' + t : 'Pharo World';
  }

  // The cursors made so far, by their pixels
  const cursors = new Map();
  function fnv(bytes) {
    let h = 0x811c9dc5;
    for (let i = 0; i < bytes.length; i++) h = Math.imul(h ^ bytes[i], 0x01000193);
    return (h >>> 0).toString(16);
  }
  function setCursor({ rgba, width, height, hotX, hotY }) {
    const bytes = new Uint8Array(rgba);
    const key = width + 'x' + height + '@' + hotX + ',' + hotY + ':' + fnv(bytes);
    let css = cursors.get(key);
    if (!css) {
      let visible = false;
      for (let i = 3; i < bytes.length; i += 4) if (bytes[i]) { visible = true; break; }
      if (!width || !height || width > 128 || height > 128) css = 'default';   // too large for browsers
      else if (!visible) css = 'none';
      else {
        const c = document.createElement('canvas');
        c.width = width;
        c.height = height;
        c.getContext('2d').putImageData(new ImageData(new Uint8ClampedArray(rgba), width, height), 0, 0);
        css = 'url(' + c.toDataURL('image/png') + ') ' + hotX + ' ' + hotY + ', default';
      }
      if (cursors.size >= 64) cursors.delete(cursors.keys().next().value);
      cursors.set(key, css);
    }
    canvas.style.cursor = css;
  }

  // The system clipboard has not the image's last copy, which the browser
  // refused to write: a paste must not bring its text over that copy
  let staleClipboard = false;
  function writeClipboard(text) {
    const g = gen;
    const refused = () => {
      if (g !== gen) return;
      staleClipboard = true;
      showNotice('The browser did not let Pharo copy to the system clipboard: the copy pastes within Pharo only.',
                 null, null, 6000);
    };
    try {
      if (!navigator.clipboard || !navigator.clipboard.writeText) { refused(); return; }
      navigator.clipboard.writeText(String(text).replace(/\r\n?/g, '\n'))
        .then(() => { if (g === gen) staleClipboard = false; }, refused);
    } catch (e) { refused(); }
  }
  // the system clipboard may have changed since: in another window, or by a
  // copy of the page (Ctrl+C in the world copies nothing of the sink's)
  addEventListener('blur', () => { staleClipboard = false; });
  for (const type of ['copy', 'cut'])
    document.addEventListener(type, e => { if (e.target !== sink) staleClipboard = false; });

  function focusSink() {
    if (document.activeElement !== sink) sink.focus({ preventScroll: true });
  }

  // ---- input

  const live = () => !!worker && alive && framed;
  const pointer = { x: 0, y: 0, buttons: 0 };
  let moveFrame = 0, moveMods = 0;

  // Whether the world took input that the image saved in this browser does
  // not have: a button, a key or a text since the VM started or last saved
  // (input during a Save that the page asked for counts after it), or
  // anything since it started once another tab saved over its image.
  // Closing the page then asks first.
  let unsaved = false, touched = false, inputWhileSaving = false;
  function noteInput() {
    unsaved = touched = true;
    if (saving) inputWhileSaving = true;
  }
  addEventListener('beforeunload', e => {
    if (!worker || !alive || !(unsaved || preparing)) return;
    e.preventDefault();
    e.returnValue = '';                 // older browsers ask for it
  });

  function send(rec) {
    if (!live()) return;
    if (rec[0] === PRESS || rec[0] === KEY_DOWN || rec[0] === TEXT) noteInput();
    worker.postMessage({ type: 'display', kind: 'event', event: rec });
  }
  // a record of OSWebDriver: type, timestamp, x, y, a, mods | buttons << 8, c, d
  function record(type, a = 0, c = 0, d = 0, mods = 0) {
    send([type, Math.round(performance.now()) | 0, pointer.x, pointer.y, a, mods | (pointer.buttons << 8), c, d]);
  }

  function flushMove() {
    if (!moveFrame) return;
    cancelAnimationFrame(moveFrame);
    moveFrame = 0;
    record(MOVE, 0, 0, 0, moveMods);
  }
  function moveTo(x, y, mods) {
    if (x === pointer.x && y === pointer.y) return;
    pointer.x = x;
    pointer.y = y;
    moveMods = mods;
    if (!moveFrame) moveFrame = requestAnimationFrame(() => { moveFrame = 0; record(MOVE, 0, 0, 0, moveMods); });
  }

  function position(e) {
    const r = canvas.getBoundingClientRect();
    return [Math.round(e.clientX - r.left), Math.round(e.clientY - r.top)];
  }

  // A pointer event: a move, or buttons that went down or up there (a
  // second button pressed while one is held comes as a pointermove)
  function onPointer(e) {
    if (!live()) return;
    const [x, y] = position(e), mods = Keymap.modifiers(e, OS), b = Keymap.buttons(e.buttons);
    moveTo(x, y, mods);
    if (b === pointer.buttons) return;
    flushMove();
    for (const [mask, n] of BUTTONS) {
      if ((b & mask) && !(pointer.buttons & mask)) {
        pointer.buttons |= mask;
        record(PRESS, n, 0, 0, mods);
      } else if (!(b & mask) && (pointer.buttons & mask)) {
        pointer.buttons &= ~mask;
        record(RELEASE, n, 0, 0, mods);
      }
    }
  }

  stage.addEventListener('pointerdown', e => {
    if (e.target !== canvas) return;
    e.preventDefault();
    focusSink();
    try { canvas.setPointerCapture(e.pointerId); } catch (_) { /* gone */ }
    onPointer(e);
  });
  stage.addEventListener('pointermove', e => { if (e.target === canvas) onPointer(e); });
  stage.addEventListener('pointerup', e => { if (e.target === canvas) onPointer(e); });
  stage.addEventListener('pointercancel', () => releaseButtons());
  stage.addEventListener('mousedown', e => { if (e.target === canvas) e.preventDefault(); });
  stage.addEventListener('contextmenu', e => { if (e.target === canvas) e.preventDefault(); });

  let wheelX = 0, wheelY = 0;
  stage.addEventListener('wheel', e => {
    if (e.target !== canvas) return;
    e.preventDefault();
    if (!live()) return;
    const unit = e.deltaMode === 1 ? LINE : e.deltaMode === 2 ? PAGE : 1;
    wheelX += e.deltaX * unit;
    wheelY += e.deltaY * unit;
    const nx = Math.trunc(wheelX / NOTCH), ny = Math.trunc(wheelY / NOTCH);
    if (!nx && !ny) return;
    wheelX -= nx * NOTCH;
    wheelY -= ny * NOTCH;
    const [x, y] = position(e), mods = Keymap.modifiers(e, OS);
    moveTo(x, y, mods);
    flushMove();
    record(WHEEL, 0, nx, -ny, mods);
  }, { passive: false });

  function releaseButtons() {
    flushMove();
    for (const [mask, n] of BUTTONS) {
      if (!(pointer.buttons & mask)) continue;
      pointer.buttons &= ~mask;
      record(RELEASE, n);
    }
  }

  // The keys held, by code: released when the page loses the focus
  const held = new Map();
  let pendingPaste = null;              // the Ctrl/Cmd+V waiting for its paste event

  // The keys of the world: those of the sink, the canvas or the page, but
  // none while a dialog shows (Images, Settings), whose focus may have
  // fallen to the body; a key held from before is still released
  const keyTarget = t => (t === sink || t === document.body || t === document.documentElement || t === canvas) &&
    !document.querySelector('dialog[open]');
  function sendKey(type, k, repeat) {
    record(type, k.sym, k.scan, repeat ? 1 : 0, k.mods);
  }

  function flushPaste(drop) {
    if (!pendingPaste) return;
    const { k, repeat, timer } = pendingPaste;
    pendingPaste = null;
    clearTimeout(timer);
    if (!drop) sendKey(KEY_DOWN, k, repeat);
  }

  // F6 leaves the world for the toolbar: its first button that works, or
  // the link to the Console
  function toToolbar() {
    ([...document.querySelectorAll('.toolbar button')].find(b => !b.disabled) || $('console')).focus();
  }

  document.addEventListener('keydown', e => {
    if (!keyTarget(e.target) || !live()) return;
    if (e.isComposing || e.keyCode === 229) return;   // the input event brings the text
    if (e.key === 'F6' && !e.ctrlKey && !e.altKey && !e.metaKey) {
      e.preventDefault();
      toToolbar();
      return;
    }
    const k = Keymap.key(e, OS);
    if (!k.sym && !k.text) return;
    if (k.prevent) e.preventDefault();
    if (e.target !== sink) focusSink();
    held.set(e.code || e.key, k);
    flushMove();
    flushPaste();
    if (!keyAt) keyAt = performance.now();
    if (k.paste) {
      pendingPaste = { k, repeat: e.repeat, timer: setTimeout(() => flushPaste(), PASTE_MS) };
      return;
    }
    sendKey(KEY_DOWN, k, e.repeat);
    if (k.text) record(TEXT, 0, k.text, 0, k.mods);
  });

  document.addEventListener('keyup', e => {
    const id = e.code || e.key;
    if (!held.has(id) && !keyTarget(e.target)) return;
    held.delete(id);
    if (!live()) return;
    const k = Keymap.key(e, OS);
    if (k.prevent) e.preventDefault();
    flushPaste();
    sendKey(KEY_UP, k, false);
  });

  // The text of a paste goes before the key that pastes it, unless the
  // image has a copy of its own that the system clipboard lacks
  sink.addEventListener('paste', e => {
    e.preventDefault();
    const text = e.clipboardData ? e.clipboardData.getData('text/plain') : '';
    if (text && live() && !staleClipboard) {
      noteInput();
      worker.postMessage({ type: 'display', kind: 'clipboard', text });
    }
    flushPaste();
  });

  // What the keys did not type: input methods, virtual keyboards
  function typed() {
    const text = sink.value;
    sink.value = '';
    if (!live()) return;
    for (const ch of text) {
      const c = ch.codePointAt(0);
      if (c >= 32 && c !== 127) record(TEXT, 0, c);
    }
  }
  sink.addEventListener('input', e => { if (!e.isComposing) typed(); });
  sink.addEventListener('compositionend', typed);

  function releaseAll(notify) {
    flushPaste(true);
    flushMove();
    for (const k of held.values()) sendKey(KEY_UP, Object.assign({}, k, { mods: 0 }), false);
    held.clear();
    releaseButtons();
    if (notify) record(FOCUS, 0);
  }

  // The world has the keyboard while the sink or the canvas has the focus.
  // In Chromium a second button pressed while one is held, a pointermove
  // that nothing can cancel, moves the focus from the sink to what was
  // pressed: the canvas, which hands it back.  Only a focus that goes
  // elsewhere, or leaves the window, releases what is held.
  let focused = false;                  // what the image was told last
  function gainedFocus() {
    if (focused) return;
    focused = true;
    record(FOCUS, 1);
  }
  function lostFocus(e) {
    if (e.relatedTarget === sink || e.relatedTarget === canvas) return;
    releaseAll(focused);
    focused = false;
  }
  sink.addEventListener('blur', lostFocus);
  sink.addEventListener('focus', gainedFocus);

  // ---- the size of the canvas
  //
  // The page sends the size of the stage in CSS pixels, the extent of the
  // world's window, and in device pixels (devicePixelRatio times that), the
  // extent the image draws in: it takes its canvasScaleFactor from the two,
  // and paints frames of devicePixelRatio times the size of the world.  The
  // canvas shows them one frame pixel per device pixel (the CSS of #world,
  // with --world-dpr).  The ratio changes when the window moves to another
  // screen, or with the zoom of the page: the sizes are sent again.

  let sentSize = null, resizeTimer = 0, observed = false;
  const ratio = () => (window.devicePixelRatio > 0 ? window.devicePixelRatio : 1);
  const stageSize = () => ({ width: stage.clientWidth, height: stage.clientHeight });
  function sendSize() {
    const { width, height } = stageSize();
    if (!worker || !width || !height) return;         // hidden: the world stays as it is
    const r = ratio(), pixelWidth = Math.round(width * r), pixelHeight = Math.round(height * r);
    if (sentSize && sentSize.width === width && sentSize.height === height &&
        sentSize.pixelWidth === pixelWidth && sentSize.pixelHeight === pixelHeight) return;
    sentSize = { width, height, pixelWidth, pixelHeight };
    worker.postMessage({ type: 'display', kind: 'resize', width, height, pixelWidth, pixelHeight });
  }
  new ResizeObserver(() => {
    clearTimeout(resizeTimer);
    if (!observed) { observed = true; sendSize(); }
    else resizeTimer = setTimeout(sendSize, RESIZE_MS);
  }).observe(stage);
  (function watchRatio() {
    const r = ratio();
    document.documentElement.style.setProperty('--world-dpr', String(r));
    const query = matchMedia('(resolution: ' + r + 'dppx)');
    query.addEventListener('change', () => { watchRatio(); sendSize(); }, { once: true });
  })();

  // ---- the toolbar

  function stop() {
    if (!worker || !alive || !ready) return;
    worker.postMessage({ type: 'interrupt' });
    const g = gen, t0 = performance.now();
    clearTimeout(watchdog);
    watchdog = setTimeout(() => {
      if (g !== gen || lastMsgAt > t0 || !alive) return;
      respawn();
    }, WATCHDOG_MS);
  }

  function save() {
    if (!live()) return;
    saving = true;
    updateStatus();
    record(SAVE_REQUEST);
  }

  // Save into a new slot, of the name asked: the worker binds the VM to it
  function saveAs() {
    if (!live()) return;
    const base = currentName || (currentEntry && currentEntry.label) || 'Pharo';
    const name = prompt('Save as a new image named', base + ' copy');
    if (name === null || !name.trim()) return;
    worker.postMessage({ type: 'saveAs', name: name.trim() });
    save();
  }

  // Rename the slot of the VM (the notice of its first save offers it)
  async function renameCurrent() {
    const sid = current.slot;
    if (!sid || !store) return;
    const name = prompt('Rename the image', currentName);
    if (name === null || !name.trim() || name.trim() === currentName) return;
    try {
      await store.rename(sid, name.trim().slice(0, 200));
      currentName = name.trim().slice(0, 200);
      showNotice("The image is now named '" + currentName + "'.", null, null, 4000);
    } catch (e) { showNotice('Could not rename it: ' + ((e && e.message) || e), null, null, 6000); }
  }

  async function download() {
    if (!worker || !ready) return;
    let files;
    try { files = await request({ type: 'download' }); }
    catch (e) { showNotice('Download failed: ' + e.message, null, null, 6000); return; }
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
    showNotice('Downloaded ' + files.map(f => f.name + ' (' + mb(f.data.length) + ')').join(' and ') + '.', null, null, 6000);
  }

  // ---- Open: an image of your own (open-image.js)

  // Open the files chosen or dropped; the VM is replaced only once they are
  // unpacked and checked.  The progress goes to the notice, which is kept
  // quiet meanwhile (aria-busy): the live region says what is unpacked, once.
  async function openFiles(files) {
    if (unavailable || opening || !files.length) return;
    opening = true;
    updateControls();
    let opened = null;
    try {
      const choice = await PharoOpen.choose(files);
      // it becomes an image of its own: only what the world did is lost
      if (live() && unsaved &&
          !confirm('Open ' + choice.name + ' as a new image? What the world did since it was last saved is lost.'))
        return;
      let shown = -1;
      const progress = (loaded, total) => {
        const pct = Math.floor(100 * loaded / Math.max(1, total));
        if (pct !== shown) showNotice('Unpacking ' + choice.from + '… ' + (shown = pct) + '%', null, null);
      };
      if (choice.from) {
        progress(0, 1);
        $('notice').setAttribute('aria-busy', 'true');
        say('Unpacking ' + choice.from);
      }
      opened = await PharoOpen.load(choice, progress);
    } catch (e) {
      const text = String((e && e.message) || e);
      // the launcher stays, and says why
      if (choosing) images.open(text, true);
      else showNotice(text, 'Open another', chooseFile);
    } finally {
      opening = false;
      $('notice').removeAttribute('aria-busy');
      updateControls();
    }
    if (!opened) return;
    // the VM runs neither the slot nor the fresh image of before: until it
    // is kept, the URL, the link to the Console and Images name none
    beforeUpload = current;
    upload = opened;
    restarted = '';
    currentName = '';
    spawn({});
  }
  const chooseFile = () => $('open-file').click();
  PharoOpen.drops(window, {
    enabled: () => !$('open').disabled,
    show: on => { $('drop').hidden = !on; },
    open: openFiles,
  });

  // ---- Images (images.js): the images of the site and those of this browser

  // Delete the slot of the VM (sid), or every slot (null): the worker
  // forgets it, and the page starts again, on the image saved last, or,
  // with none, as on a first visit
  async function deleteRunning(sid) {
    const msg = sid ? { type: 'resetStorage', slot: sid } : { type: 'resetStorage', all: true };
    if (worker) await request(msg);
    else if (store) await (sid ? store.remove(sid) : store.reset());
    upload = null;
    restarted = sid ? "Deleted '" + (currentName || 'the image') + "' from this browser." : 'Deleted every image saved in this browser.';
    images.close();
    const left = store ? await store.list().catch(() => []) : [];
    if (!left.length && Catalog.images(manifest).length >= 2) launch(restarted);
    else spawn({});
  }

  // The image chosen in Images: after a question when the world has input
  // that is not saved
  function openChoice(choice) {
    const entry = choice.fresh && manifest ? Catalog.find(manifest, choice.fresh) : null;
    const what = entry ? 'a fresh ' + entry.label : 'that image';
    if (live() && unsaved && !confirm('Open ' + what + '? What the world did since it was last saved is lost.')) return false;
    upload = null;
    restarted = '';
    spawn(choice);
    return true;
  }

  // The launcher: the Images dialog that cannot be closed, no VM
  function launch(text) {
    kill();
    choosing = true;
    setChoice({});
    setStatus('choosing', 'Choose an image');
    say('Choose a Pharo image');
    showOverlay('Choose a Pharo image', false);
    hideNotice();
    updateControls();
    images.open(text || null, true);
  }

  // ---- the git proxy: Settings, and what the worker says of git's requests

  const gitSettings = GitProxy.wireDialog({
    defaultProxy: () => (manifest && manifest.gitProxyDefault) || '',
    onSaved: (value, chosen, kept) => {
      if (worker) worker.postMessage({ type: 'gitProxy', gitProxy: value });
      const origin = chosen.mode === 'default' ? GitProxy.originOf((manifest && manifest.gitProxyDefault) || '')
        : GitProxy.originOf(chosen.value);
      showNotice((origin ? 'Git requests now go through the proxy at ' + origin +
                           (chosen.mode === 'default' ? " (this site's default)." : '.')
                         : 'Git requests now go to the repositories themselves, without a proxy.') +
                 (kept ? '' : ' This browser cannot keep the setting: it holds for this page only.'), null, null, 8000);
    },
    refocus: () => refocus(),
  });
  const SETTINGS = 'Settings';
  function onGit(m) {
    const chosen = GitProxy.read(), proxy = m.proxy || '';
    if (m.event === 'request') {
      if (chosen.mode === 'default' && proxy && !GitProxy.noted(proxy)) {
        GitProxy.markNoted(proxy);
        showNotice('Git requests (Iceberg clones, Metacello github://) go through the public CORS proxy ' + proxy +
                   ', which sees what you clone.', SETTINGS, gitSettings.open, 15000);
      }
    } else if (m.event === 'failed') {
      if (!proxy && m.status < 0)
        showNotice("The browser blocked git's request to " + m.host + ' (CORS): git over HTTPS needs a proxy here.',
                   SETTINGS, gitSettings.open, 15000);
      else if (proxy && m.status < 0)
        showNotice('The git proxy ' + proxy + ' did not answer.', SETTINGS, gitSettings.open, 15000);
      else if (proxy && m.status === 403)
        showNotice('The git proxy ' + proxy + ' refused the request to ' + m.host + '.', SETTINGS, gitSettings.open, 15000);
    }
  }

  const refocus = () => { if (framed) focusSink(); };
  $('stop').addEventListener('click', () => { stop(); refocus(); });
  $('save').addEventListener('click', () => { save(); refocus(); });
  $('save-as').addEventListener('click', () => { saveAs(); refocus(); });
  $('images').addEventListener('click', () => { if (images) images.open(); });
  $('settings').addEventListener('click', () => gitSettings.open());
  $('download').addEventListener('click', () => { download(); refocus(); });
  $('open').addEventListener('click', chooseFile);
  $('open-file').addEventListener('change', e => {
    const files = [...e.target.files];
    e.target.value = '';
    openFiles(files);
  });
  $('notice-action').addEventListener('click', () => { const f = noticeAction; hideNotice(); if (f) f(); refocus(); });
  $('notice-close').addEventListener('click', () => { hideNotice(); refocus(); });

  // The worker stores the .changes of the saved image soon after the world
  // idles; when the page goes away, it is asked to store what is left, which
  // it may not live to do
  const flush = () => { if (worker) worker.postMessage({ type: 'flush' }); };
  addEventListener('pagehide', flush);
  document.addEventListener('visibilitychange', () => { if (document.visibilityState === 'hidden') flush(); });

  window.PharoWorld = {
    stats,
    get state() { return statusPill.dataset.state; },
    get unsaved() { return !!worker && alive && unsaved; },
    get choice() { return Object.assign({}, current); },
    get images() { return images; },
    readFile: path => request({ type: 'fs', op: 'readFile', path }).then(r => r.data),
    fs: (op, path, fields) => request(Object.assign({ type: 'fs', op, path }, fields || {})),
  };

  // ---- start

  let consoleWorks = false;
  if (location.protocol === 'file:')
    unavailable = HTTP_HINT + '\nBrowsers do not run workers or fetch .wasm files from file:// URLs.';
  else if (typeof WebAssembly !== 'object' || typeof Worker !== 'function')
    unavailable = 'This browser lacks WebAssembly or Web Workers.';
  else if (typeof DecompressionStream !== 'function')
    unavailable = 'This browser cannot inflate the files of Pharo (it has no DecompressionStream): use a newer one.';
  else if (typeof HTMLCanvasElement.prototype.transferControlToOffscreen !== 'function') {
    unavailable = 'This browser cannot draw from a worker (it has no OffscreenCanvas), which the world needs; the Console works.';
    consoleWorks = true;
  }
  if (unavailable) fatal(unavailable, consoleWorks ? 'Open the Console' : null, 'index.html');
  else start();

  // The image of the URL (?slot=, ?fresh=: nothing else of it is read),
  // else the last one saved or booted, else, on a first visit to a site of
  // several images, the launcher
  async function start() {
    const q = new URLSearchParams(location.search);
    const slot = q.get('slot'), fresh = q.get('fresh');
    const want = slot && Storage.SLOT_ID.test(slot) ? { slot } : fresh && Catalog.ID.test(fresh) ? { fresh } : {};
    showOverlay('Loading Pharo…', null);
    let storageUnavailable = !store;
    const [m, slots] = await Promise.all([
      fetch('manifest.json' + Q).then(r => r.ok ? r.json() : null).catch(() => null),
      store ? store.list().catch(() => { storageUnavailable = true; return []; }) : Promise.resolve([]),
    ]);
    manifest = m;
    $('settings').hidden = !(manifest && manifest.git && manifest.gitHttp);
    images = PharoImages.create({
      store: storageUnavailable ? null : store,
      manifest: () => manifest,
      current: () => current,
      running: () => (worker && current.slot) || null,
      onOpen: openChoice,
      onOpenFile: chooseFile,
      onDeleteRunning: deleteRunning,
      // a rename of the slot of the VM
      onChange: () => {
        if (current.slot && store) store.name(current.slot).then(n => { if (n) currentName = n; }, () => {});
      },
      otherPage: { label: 'Console', href: c => 'index.html' + query(c) },
    });
    if (!manifest) spawn(want);           // (the worker says why it cannot start)
    else if (want.slot) {
      if (slots.some(s => s.slot === want.slot)) spawn(want);
      else launch('That saved image is not in this browser.');
    } else if (want.fresh) spawn(want);
    else if (slots.length) spawn({});
    else if (Catalog.images(manifest).length >= 2) launch();
    else spawn({});
  }
})();
