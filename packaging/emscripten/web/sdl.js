// sdl.js - the SDL2 page (sdl.html) of the Pharo VM for WebAssembly
//
// The page side of the world through SDL2, on a build with WASM_SDL2=ON:
// owns the VM's worker (vm-worker.js, see there for the message protocol) in
// 'sdl' mode, whose image opens its world through its own OSSDL2Driver.
// SDL2's Emscripten video driver draws it into the canvas of this page,
// transferred to the worker as an OffscreenCanvas, through the DOM shim of
// sdl-shim.js.  No dependencies but open-image.js.
//
// - pharo-web.wasm is compiled once here and handed to every worker, so a
//   restart only instantiates it.  A canvas can be transferred only once:
//   every worker gets a new one.  A generation number per worker drops the
//   messages of a worker that was replaced.
// - The canvas starts at the size of the stage, which the shim makes the
//   screen of SDL; then SDL's window gives it its size (sdl {size}), which
//   the page shows one canvas pixel per CSS pixel, scrolling the stage when
//   it is larger.  The page does not resize it: SDL listens for that on the
//   window, which a worker has not.
// - Events: the DOM events of the canvas go to the worker as records of the
//   fields that Emscripten's html5 library reads, with clientX and clientY
//   relative to the canvas (sdl {event}); the shim dispatches them where SDL
//   listens.  Mouse: mousedown, mouseenter and mouseleave on the canvas, the
//   moves once per animation frame (the last one, and before any other
//   event), the wheel, and mouseup anywhere in the page, which SDL takes
//   from the document.  The context menu is the image's.  Keys: keydown,
//   keypress and keyup of the focused canvas.  Their defaults are prevented,
//   but for a key down that types a character: the browser then sends the
//   keypress, which SDL turns into SDL_TEXTINPUT, the text that the image
//   inserts.  Shortcuts with Ctrl or Cmd (Ctrl+P is the world's print-it,
//   not the browser's printing) are kept from the browser.  F6 moves the
//   keyboard to the toolbar, and Tab from there back into the world.
// - The title and the cursor of SDL go to the page; a cursor image that SDL
//   made (SDL_CreateCursor) comes as its pixels (sdl {cursorImage}), and the
//   page makes its data URL.
// - Any image runs: the build's (its OSWindow-Web, if it has it, gives the
//   world to SDL without a display), or one that Open (or files dropped on
//   the page) gives, stock images too.  Nothing is kept in this browser:
//   the image that the world menu's Save writes stays in the memory of the
//   VM, which Download gives.  Restart boots the same image again.
// - Not bridged: the system clipboard (SDL2 has no Emscripten clipboard:
//   copy and paste stay within the image), resizing, HiDPI (a canvas pixel
//   is a CSS pixel), input methods, focus and Stop (the image registers
//   nothing for it with the VM: Alt+. or Cmd+. interrupts it).
//
// window.PharoSDL tells tests and the curious how it goes: stats (the
// workers started, when this one started and painted first, the sizes of
// its canvas, the title, the cursors), state (that of the status pill),
// readFile(path), a file of the VM, and costs(), what the worker measured:
// {slices, sliceMs, presents, presentMs, pixels, firstPresentMs}.

(function () {
  'use strict';

  const RUNNING = 0, WAITING = 1, BUSY = 2, SLEEPING = 4;
  const BUSY_MS = 1000;                 // busy, when the VM has not slept for so long
  const START_MS = 60000;               // the world paints so long after ready, or did not open
  const REVOKE_MS = 60000;              // a download has long begun then: its Blob may go
  const CURSORS = 64;                   // the cursor images kept
  const CURSOR_SCHEME = 'pharo-sdl-cursor:';

  const BUILD = (document.querySelector('meta[name="pharo-build"]') || {}).content || '';
  const Q = BUILD && BUILD.indexOf('@') < 0 ? '?v=' + encodeURIComponent(BUILD) : '';
  const $ = id => document.getElementById(id);
  const scroller = $('scroller'), statusPill = $('status'), statusText = $('status-text');
  let canvas = $('screen');

  // The theme of the Console page, when it chose one
  try {
    const theme = localStorage.getItem('pharo-wasm.theme');
    if (theme === 'light' || theme === 'dark') document.documentElement.dataset.theme = theme;
  } catch (e) { /* no storage: follow the system */ }

  let worker = null, gen = 0, ready = false, alive = false, painted = false, state = null;
  let idleAt = 0, lastMsgAt = 0, startTimer = 0, loadedText = 'Loading';
  let unavailable = null;               // why the page cannot run the world at all
  let restarted = '';                   // what the next ready says first
  let upload = null;                    // {name, image, changes, sources}: the image opened, booted by every restart
  let opening = false;                  // Open unpacks files
  const reqs = new Map();
  let reqId = 0;
  // the workers started; when this one started and first painted; the sizes
  // that SDL gave the canvas; the title; the cursors set
  const stats = { starts: 0, startedAt: 0, paintedAt: 0, sizes: [], title: '', cursors: 0 };

  // ---- status, overlay and notice

  function setStatus(key, text) {
    statusPill.dataset.state = key;
    if (statusText.textContent !== text) statusText.textContent = text;
    if (key === 'loading') say('Loading Pharo');
    else if (key === 'starting') say('Starting Pharo');
    else if (key === 'running' && said === 'Starting Pharo') say('The Pharo world is running');
    else if (key === 'exited' || key === 'crashed' || key === 'error') say(text);
  }
  let said = '';
  function say(text) {
    if (text !== said) $('status-live').textContent = said = text;
  }
  function updateStatus() {
    if (!alive) return;
    if (!ready) return setStatus('loading', loadedText);
    if (!painted) return setStatus('starting', 'Starting');
    if (state === BUSY && performance.now() - idleAt > BUSY_MS) return setStatus('busy', 'Busy');
    return setStatus('running', 'Running');
  }
  setInterval(updateStatus, BUSY_MS / 4);
  function updateControls() {
    $('restart').disabled = !!unavailable || !worker;
    $('download').disabled = !(worker && ready);
    $('open').disabled = !!unavailable || opening;
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
  // to the page action, if any
  let noticeAction = null, noticeTimer = 0;
  function showNotice(text, label, action, ms) {
    clearTimeout(noticeTimer);
    $('notice-text').textContent = text;
    const link = typeof action === 'string';
    $('notice-action').hidden = !label || link;
    $('notice-action').textContent = label && !link ? label : '';
    $('notice-link').hidden = !label || !link;
    $('notice-link').textContent = label && link ? label : '';
    if (link) $('notice-link').href = action;
    noticeAction = link ? null : action || null;
    $('notice').hidden = false;
    if (ms) noticeTimer = setTimeout(hideNotice, ms);
  }
  function hideNotice() {
    clearTimeout(noticeTimer);
    $('notice').hidden = true;
    noticeAction = null;
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
    clearTimeout(startTimer);
    if (moveFrame) cancelAnimationFrame(moveFrame);
    moveFrame = 0;
    pendingMove = null;
    if (worker) worker.terminate();
    worker = null;
    alive = ready = painted = false;
    state = null;
    for (const r of reqs.values()) r.reject(new Error('the VM was restarted'));
    reqs.clear();
    cursorImages.clear();
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
  // one, of the size of the stage, with the listeners of the page
  function freshCanvas(width, height) {
    const c = document.createElement('canvas');
    c.id = 'screen';
    c.setAttribute('aria-label', canvas.getAttribute('aria-label'));
    c.tabIndex = 0;
    c.width = width;
    c.height = height;
    c.style.width = width + 'px';
    c.style.height = height + 'px';
    c.style.cursor = 'default';
    listen(c);
    canvas.replaceWith(c);
    canvas = c;
    return c;
  }

  function stageSize() {
    return { width: Math.max(1, Math.floor(scroller.clientWidth)), height: Math.max(1, Math.floor(scroller.clientHeight)) };
  }

  async function spawn() {
    kill();
    const g = gen;
    alive = true;
    loadedText = 'Loading';
    stats.starts++;
    stats.startedAt = performance.now();
    stats.paintedAt = 0;
    stats.sizes = [];
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
    const c = freshCanvas(width, height);
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
      mode: 'sdl',
      persist: false,
      upload: upload || undefined,
      canvas: offscreen,
    }, [offscreen]);
    updateControls();
  }

  // The VM ended; the worker stays, for Download
  function ended(status, text, notice) {
    clearTimeout(startTimer);
    alive = false;
    restarted = '';
    setStatus(status, text);
    if (!painted) showOverlay(notice, false);
    updateControls();
    showNotice(notice, 'Restart', spawn, 0);
  }

  function onReady(m) {
    ready = true;
    if (!m.sdl2) {
      fatal('This build has no SDL2: build it with WASM_SDL2=ON for this page. The world page runs the world without it.',
            'Open the world page', 'world.html');
      return;
    }
    const notes = [restarted];
    restarted = '';
    if (m.source === 'upload') notes.push('Started the opened image, ' + (upload ? upload.name : m.image) + '.');
    const note = notes.filter(Boolean).join(' ');
    if (note) showNotice(note, null, null, 6000);
    showOverlay('Starting Pharo…', null);
    const g = gen;
    startTimer = setTimeout(() => {
      if (g !== gen || painted || !alive) return;
      showNotice('The world has not opened yet: this image may not open it through SDL2. The Console can run it.',
                 'Open the Console', 'index.html', 0);
    }, START_MS);
    updateControls();
    updateStatus();
  }

  // What the image writes on stdout and stderr goes to the browser's console
  function log(fd, text) {
    const t = text.replace(/\n$/, '');
    if (fd === 2) console.warn('[pharo] ' + t); else console.log('[pharo] ' + t);
  }

  function onWorker(m) {
    lastMsgAt = performance.now();
    switch (m.type) {
    case 'progress': {
      const fraction = m.total ? m.loaded / m.total : 1;
      loadedText = m.phase === 'boot' ? 'Starting' : 'Loading ' + Math.floor(100 * fraction) + '%';
      if (m.phase === 'boot') showOverlay('Starting Pharo…', null);
      else showOverlay('Downloading Pharo… ' + Math.floor(100 * fraction) + '%', fraction);
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
    case 'saved':
      // no storage here: the worker says that the image is kept nowhere
      showNotice('The image saved itself (' + mb(m.bytes) + '), in the memory of this page only: Download keeps it.',
                 'Download', download, 10000);
      break;
    case 'exit':
      ended('exited', 'Exited (' + m.code + ')', 'Pharo quit with exit code ' + m.code + '.');
      break;
    case 'crash':
      console.warn('pharo: ' + m.message + (m.stacks ? '\n' + m.stacks : '') + (m.stack ? '\n' + m.stack : ''));
      ended('crashed', 'Crashed', 'The VM crashed: ' + String(m.message).split('\n')[0]);
      break;
    case 'sdl':
      onSDL(m);
      break;
    case 'fs-result': case 'file': case 'error': case 'stats': {
      const r = reqs.get(m.id);
      if (!r) break;
      if (m.type === 'file') {           // one message per file
        r.files.push(m);
        if (r.files.length < m.count) break;
      }
      reqs.delete(m.id);
      if (m.type === 'error') r.reject(new Error(m.message));
      else r.resolve(m.type === 'file' ? r.files : m.type === 'stats' ? m.stats : m);
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

  // ---- what SDL asks for

  // The cursor images of SDL_CreateCursor, as CSS URLs, by their number
  const cursorImages = new Map();

  function onSDL(m) {
    switch (m.kind) {
    case 'painted':
      if (painted) break;
      painted = true;
      stats.paintedAt = performance.now();
      clearTimeout(startTimer);
      hideOverlay();
      canvas.focus({ preventScroll: true });
      updateStatus();
      break;
    case 'size': {
      const width = Math.max(1, m.width | 0), height = Math.max(1, m.height | 0);
      stats.sizes.push([width, height]);
      // (the drawing buffer is the worker's: only the size it shows at)
      canvas.style.width = width + 'px';
      canvas.style.height = height + 'px';
      break;
    }
    case 'title': {
      let t = String(m.title || '');
      if (t.startsWith('/')) t = t.slice(t.lastIndexOf('/') + 1);   // the image's path
      stats.title = t;
      document.title = t ? 'Pharo - ' + t + ' (SDL2)' : 'Pharo World (SDL2)';
      break;
    }
    case 'cursorImage':
      cursorImages.set(m.id, cursorUrl(m));
      while (cursorImages.size > CURSORS) cursorImages.delete(cursorImages.keys().next().value);
      break;
    case 'cursor':
      stats.cursors++;
      canvas.style.cursor = cssCursor(String(m.cursor || ''));
      break;
    }
  }

  // The data URL of the pixels of a cursor image, or '' when it shows nothing
  function cursorUrl({ width, height, rgba }) {
    const bytes = new Uint8ClampedArray(rgba);
    if (!width || !height || width > 128 || height > 128 || bytes.length !== width * height * 4) return '';
    const c = document.createElement('canvas');
    c.width = width;
    c.height = height;
    c.getContext('2d').putImageData(new ImageData(bytes, width, height), 0, 0);
    return c.toDataURL('image/png');
  }
  // SDL's CSS cursor, with the URLs of its images: an image that is gone,
  // or too large for a cursor, leaves the fallback after it
  function cssCursor(css) {
    return css.replace(/url\(pharo-sdl-cursor:(\d+)\)(\s+\d+\s+\d+)?\s*,\s*/g, (all, id, hot) => {
      const url = cursorImages.get(Number(id));
      return url ? 'url(' + url + ')' + (hot || '') + ', ' : '';
    }) || 'default';
  }

  // ---- input

  const live = () => !!worker && alive && ready;
  // The fields of the DOM events that html5 reads
  const MOUSE = ['screenX', 'screenY', 'button', 'buttons', 'movementX', 'movementY',
                 'ctrlKey', 'shiftKey', 'altKey', 'metaKey'];
  const WHEEL = MOUSE.concat(['deltaX', 'deltaY', 'deltaZ', 'deltaMode']);
  const KEY = ['key', 'code', 'keyCode', 'charCode', 'which', 'location', 'repeat',
               'ctrlKey', 'shiftKey', 'altKey', 'metaKey'];

  function send(rec) {
    if (!live()) return;
    worker.postMessage({ type: 'sdl', event: rec });
  }
  // A record of e: its type, timeStamp, the fields, and with mouse
  // coordinates, clientX and clientY in pixels of the canvas
  function record(e, fields, at) {
    const rec = { type: e.type, timeStamp: e.timeStamp };
    for (const f of fields) if (e[f] !== undefined) rec[f] = e[f];
    if (at) {
      const r = canvas.getBoundingClientRect();
      const sx = r.width ? canvas.offsetWidth / r.width : 1, sy = r.height ? canvas.offsetHeight / r.height : 1;
      rec.clientX = Math.round((e.clientX - r.left) * sx);
      rec.clientY = Math.round((e.clientY - r.top) * sy);
    }
    return rec;
  }

  // the moves, once per frame: the last one, sent before any other event
  let moveFrame = 0, pendingMove = null;
  function flushMove() {
    if (moveFrame) cancelAnimationFrame(moveFrame);
    moveFrame = 0;
    if (pendingMove) send(pendingMove);
    pendingMove = null;
  }
  function onMove(e) {
    if (!live()) return;
    pendingMove = record(e, MOUSE, true);
    if (!moveFrame) moveFrame = requestAnimationFrame(() => { moveFrame = 0; flushMove(); });
  }
  function onMouse(e) {
    if (!live()) return;
    flushMove();
    send(record(e, MOUSE, true));
    if (e.type === 'mousedown') {
      e.preventDefault();               // no selection, no drag: but the focus
      canvas.focus({ preventScroll: true });
    }
  }
  function onWheel(e) {
    if (!live()) return;
    e.preventDefault();
    flushMove();
    send(record(e, WHEEL, true));
  }
  // Whether a key down types a character, whose keypress must then come
  // (Ctrl and Cmd make shortcuts of them; AltGr is Ctrl+Alt on Windows)
  const types = e => [...(e.key || '')].length === 1 && !e.metaKey && !(e.ctrlKey && !e.altKey);
  function onKey(e) {
    if (e.type === 'keydown' && e.key === 'F6') {
      e.preventDefault();
      $('restart').disabled ? $('open').focus() : $('restart').focus();
      return;
    }
    if (!live()) return;
    flushMove();
    send(record(e, KEY, false));
    if (!(e.type === 'keydown' && types(e))) e.preventDefault();
  }

  function listen(c) {
    c.addEventListener('mousemove', onMove);
    for (const type of ['mousedown', 'mouseenter', 'mouseleave']) c.addEventListener(type, onMouse);
    c.addEventListener('wheel', onWheel, { passive: false });
    for (const type of ['keydown', 'keypress', 'keyup']) c.addEventListener(type, onKey);
    c.addEventListener('contextmenu', e => e.preventDefault());
  }
  // SDL takes the release of a button from the document: anywhere
  addEventListener('mouseup', onMouse);

  // ---- the toolbar

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

  function restart() {
    if (unavailable) return;
    if (live() && painted && !confirm('Start the VM again? What the world did since it started is lost.')) return;
    restarted = 'VM restarted.';
    spawn();
  }

  // ---- Open: an image of your own (open-image.js)

  // Open the files chosen or dropped; the VM is replaced only once they are
  // unpacked and checked
  async function openFiles(files) {
    if (unavailable || opening || !files.length) return;
    opening = true;
    updateControls();
    let opened = null;
    try {
      const choice = await PharoOpen.choose(files);
      if (live() && painted && !confirm('Open ' + choice.name + '? What the world did since it started is lost.')) return;
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
      showNotice(String((e && e.message) || e), 'Open another', chooseFile);
    } finally {
      opening = false;
      $('notice').removeAttribute('aria-busy');
      updateControls();
    }
    if (!opened) return;
    upload = opened;
    restarted = '';
    spawn();
  }
  const chooseFile = () => $('open-file').click();
  PharoOpen.drops(window, {
    enabled: () => !$('open').disabled,
    show: on => { $('drop').hidden = !on; },
    open: openFiles,
  });

  const refocus = () => { if (painted) canvas.focus({ preventScroll: true }); };
  $('restart').addEventListener('click', restart);
  $('download').addEventListener('click', () => { download(); refocus(); });
  $('open').addEventListener('click', chooseFile);
  $('open-file').addEventListener('change', e => {
    const files = [...e.target.files];
    e.target.value = '';
    openFiles(files);
  });
  $('notice-action').addEventListener('click', () => { const f = noticeAction; hideNotice(); if (f) f(); refocus(); });
  $('notice-close').addEventListener('click', () => { hideNotice(); refocus(); });

  window.PharoSDL = {
    stats,
    get state() { return statusPill.dataset.state; },
    readFile: path => request({ type: 'fs', op: 'readFile', path }).then(r => r.data),
    costs: () => request({ type: 'stats' }),
  };

  // ---- start

  if (location.protocol === 'file:')
    unavailable = HTTP_HINT + '\nBrowsers do not run workers or fetch .wasm files from file:// URLs.';
  else if (typeof WebAssembly !== 'object' || typeof Worker !== 'function')
    unavailable = 'This browser lacks WebAssembly or Web Workers.';
  else if (typeof DecompressionStream !== 'function')
    unavailable = 'This browser cannot inflate the files of Pharo (it has no DecompressionStream): use a newer one.';
  else if (typeof HTMLCanvasElement.prototype.transferControlToOffscreen !== 'function')
    unavailable = 'This browser cannot draw from a worker (it has no OffscreenCanvas), which SDL2 needs here.';
  if (unavailable) fatal(unavailable, 'Open the Console', 'index.html');
  else spawn();
})();
