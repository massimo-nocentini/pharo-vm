// world.js - the world page (world.html) of the Pharo VM for WebAssembly
//
// The page side of the Pharo world: owns the VM's worker (vm-worker.js, see
// there for the message protocol) in 'world' mode, whose display
// (display-worker.js) draws the world into the canvas of this page, which is
// transferred to it as an OffscreenCanvas.  The page sends the display the
// size of the canvas and its input, as the event records of the image's
// OSWebDriver (packaging/emscripten/st/OSWindow-Web), and shows what the
// display asks for.  No dependencies but keymap.js.
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
// - Save asks the image to save itself (a save request record); the worker
//   keeps the saved image in this browser, and the next visit, here or in
//   the Console, boots it.  Download gives the image and its .changes.
// - The display's title, cursor (RGBA, as a CSS cursor), clipboard
//   (navigator.clipboard.writeText) and focus.  When the browser refuses to
//   write the clipboard, the system clipboard still has older text, which
//   the next paste would bring over the image's own copy: the page then
//   keeps the text of its pastes from the image until it lost the focus to
//   another window, or a copy of the page changed the clipboard.
// - A world that did not paint after START_MS, or a VM that ended before
//   it painted, may be an image that cannot open the world: the notice
//   points to the Console, which can prepare it, or reset it.
// - The status pill is no live region: the one next to it says how the VM
//   goes (loading, starting, running, ended), not how far the loading got.
//
// window.PharoWorld tells tests and the curious how it goes: stats (the
// workers started, the frames of this one, when they came and the size of
// the first, the times from a key to the next frame), state (that of the
// status pill), unsaved (whether closing the page would ask) and
// readFile(path), a file of the VM.

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

  // ---- status, overlay and notice

  let worker = null, gen = 0, ready = false, alive = false, framed = false, state = null;
  let lastMsgAt = 0, idleAt = 0, watchdog = 0, saving = false, loadedText = 'Loading', startTimer = 0;
  let heardAt = 0, pingAt = 0;          // when the worker last said anything, and was asked
  let restarted = '';                   // why the worker was replaced, said until the next one is ready
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
    const on = !!worker && alive && ready;
    $('stop').disabled = !on;
    $('save').disabled = !(on && framed);
    $('download').disabled = !(worker && ready);
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
  const CONSOLE = { label: 'Open the Console', href: 'index.html' };

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
    alive = ready = framed = saving = false;
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

  async function spawn() {
    kill();
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
    c.width = Math.max(1, width);
    c.height = Math.max(1, height);
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
      display: { canvas: offscreen, width, height },
    }, [offscreen]);
    // queued by the worker until it is ready, so before the image boots,
    // which then opens the world at the size of the canvas
    sentSize = null;
    sendSize();
  }

  // The VM ended; the worker stays, for Download.  Before the world
  // painted, the image may be one that cannot open it: the Console can
  // prepare it, or reset it
  function ended(status, text, notice) {
    clearTimeout(watchdog);
    clearTimeout(startTimer);
    alive = false;
    restarted = '';
    saving = false;
    releaseAll(false);
    sink.tabIndex = -1;
    setStatus(status, text);
    if (!framed) showOverlay(notice, false);
    updateControls();
    showNotice(notice, 'Restart', spawn, 0, framed ? null : CONSOLE);
  }

  function respawn() {
    restarted = 'VM restarted (unsaved changes lost).';
    spawn();
  }

  function onReady(m) {
    ready = true;
    if (!m.world) {
      fatal('This build has no world image: build it with WASM_WORLD=ON and a host Pharo (WASM_HOST_PHARO).',
            'Open the Console', 'index.html');
      return;
    }
    if (!m.prepared) {
      fatal('The image saved in this browser cannot open the Pharo world yet: in the Console, ' +
            'choose "Prepare for the world", which saves it prepared.', 'Open the Console', 'index.html');
      return;
    }
    const notes = [restarted];
    restarted = '';
    if (m.source === 'saved')
      notes.push('Started the image saved in this browser on ' + new Date(m.savedAt).toLocaleString() + '.');
    if (m.storageError) notes.push('Note: ' + m.storageError + '.');
    const note = notes.filter(Boolean).join(' ');
    if (note) showNotice(note, null, null, m.storageError ? 10000 : 6000);
    showOverlay('Starting Pharo…', null);
    const g = gen;
    startTimer = setTimeout(() => {
      if (g !== gen || framed || !alive) return;
      notStarted = true;
      showNotice('The world has not opened yet: this image may not open it. In the Console, choose ' +
                 '"Prepare for the world", or Reset the saved image.', 'Open the Console', 'index.html', 0);
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
    if (m.type !== 'interrupted' && m.type !== 'fs-result' && m.type !== 'file' && m.type !== 'error')
      lastMsgAt = heardAt;
    switch (m.type) {
    case 'progress': {
      const fraction = m.loaded / Math.max(1, m.total);
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
      saving = false;
      if (m.error) showNotice('Saved, but ' + m.error + '.', 'Download', download);
      else {
        unsaved = inputWhileSaving;
        showNotice('The image is saved in this browser (' + mb(m.bytes) + ').', null, null, 6000);
      }
      inputWhileSaving = false;
      updateStatus();
      break;
    case 'superseded':
      // what this VM did since it started is kept nowhere now
      unsaved = touched;
      showNotice('Another tab has replaced the image saved in this browser. Save to keep this session instead.',
                 'Save', save);
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
    case 'fs-result': case 'file': case 'error': {
      const r = reqs.get(m.id);
      if (!r) break;
      if (m.type === 'file') {           // one message per file
        r.files.push(m);
        if (r.files.length < m.count) break;
      }
      reqs.delete(m.id);
      if (m.type === 'error') r.reject(new Error(m.message));
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
    if (!worker || !alive || !unsaved) return;
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

  const keyTarget = t => t === sink || t === document.body || t === document.documentElement || t === canvas;
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

  let sentSize = null, resizeTimer = 0, observed = false;
  const stageSize = () => ({ width: stage.clientWidth, height: stage.clientHeight });
  function sendSize() {
    const { width, height } = stageSize();
    if (!worker || !width || !height) return;         // hidden: the world stays as it is
    if (sentSize && sentSize.width === width && sentSize.height === height) return;
    sentSize = { width, height };
    worker.postMessage({ type: 'display', kind: 'resize', width, height });
  }
  new ResizeObserver(() => {
    clearTimeout(resizeTimer);
    if (!observed) { observed = true; sendSize(); }
    else resizeTimer = setTimeout(sendSize, RESIZE_MS);
  }).observe(stage);

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

  const refocus = () => { if (framed) focusSink(); };
  $('stop').addEventListener('click', () => { stop(); refocus(); });
  $('save').addEventListener('click', () => { save(); refocus(); });
  $('download').addEventListener('click', () => { download(); refocus(); });
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
    readFile: path => request({ type: 'fs', op: 'readFile', path }).then(r => r.data),
  };

  // ---- start

  let unavailable = null, consoleWorks = false;
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
  else spawn();
})();
