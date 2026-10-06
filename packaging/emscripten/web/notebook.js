// notebook.js - the Notebook tab of the Console page (index.html) of the Pharo VM for WebAssembly
//
// Cells of Smalltalk and Markdown, run by a second VM of their own (the
// notebook kernel: st/web-notebook.st in a vm-worker.js in 'notebook' mode,
// driven by nb-kernel.js), separate from the Console's.  A port of CHICKEN's
// notebook.js (chicken-core, emscripten/web), with its structure and names.
// No dependencies but the scripts that index.html loads before it.
//
// - page.js provides window.PharoPage (storage, the compiled .wasm, the
//   worker's URL, the git proxy, the theme, the tabs and the pharo:* events);
//   notebook-lib.js (NotebookLib) the Markdown renderer, the markup sanitizer
//   and the .json format; notebook-st.js (NotebookSt) what is Smalltalk: the
//   bracket balance, the .st format, quote straightening and the example;
//   nb-kernel.js (PharoNotebookKernel) the kernel client.
// - The kernel starts on the first visit to the tab (or the first run).  It
//   boots the image saved in this browser, read-only, else the site's, and
//   never saves; when the Console's image changes, a notice offers Restart.
// - Kernel output is queued per cell and rendered once per animation frame,
//   or soon while the page is hidden (no frames come then); while a cell
//   runs, its outputs keep their first and last 512 KB of streams and 500
//   outputs (a note says how much was left out).  What processes forked by
//   an earlier cell write later goes to that cell ('late').
// - Rich outputs (HTML, SVG, Markdown) are rebuilt from an allowlist by
//   notebook-lib.js; no untrusted string ever reaches innerHTML.  Images come
//   as PNG, made by nb-kernel.js from the pixels of the kernel's Forms.
// - Cells have no stdin: a cell that waits for input says so, with Stop.
// - The notebook is saved to localStorage (debounced), under a key of the
//   site's directory (pharo-wasm.notebook:<dir>), outputs capped at 64 KB per
//   cell; it can be exported as .st (chunks marked "%%") or .json.
// - Files uploaded (More, or dropped on the tab) go into the kernel's /pharo,
//   and again into every new kernel.

(function () {
  'use strict';

  const $ = id => document.getElementById(id);
  const panel = $('panel-notebook');
  if (!panel) return;
  const P = window.PharoPage, L = window.NotebookLib, S = window.NotebookSt, K = window.PharoNotebookKernel;

  const STREAM_HEAD = 512 * 1024, STREAM_TAIL = 512 * 1024, OUT_HEAD = 500, OUT_TAIL = 500;
  const VALUE_MAX = 64 * 1024, VALUE_LINES = 30;
  const DISPLAY_MAX = 4 * 1024 * 1024;
  const SAVE_OUT_MAX = 64 * 1024, SAVE_MAX = 2 * 1024 * 1024;
  const TRASH_MAX = 20, LOG_LINES = 500;
  const HOME = '/pharo/';               // the kernel's working directory, where uploads go
  const SVG_NS = 'http://www.w3.org/2000/svg';

  const cellsEl = $('nb-cells');
  const statusEl = $('nb-status'), statusText = $('nb-status-text');

  function fail(msg) {
    $('nb-notice-text').textContent = msg;
    $('nb-notice').hidden = false;
    for (const id of ['nb-run-all', 'nb-stop', 'nb-restart']) { const b = $(id); if (b) b.disabled = true; }
    for (const act of ['restart-run', 'stop-background']) {
      const b = document.querySelector('#nb-menu [data-act="' + act + '"]');
      if (b) b.setAttribute('aria-disabled', 'true');
    }
    statusEl.dataset.state = 'error';
    statusText.textContent = 'unavailable';
  }
  if (!P || !L || !S) { fail('The notebook could not start: a page script did not load. Reload the page.'); return; }

  // the autosave, per site directory as the saved image is (two sites of
  // one origin keep two notebooks)
  const SAVE_KEY = 'pharo-wasm.' + P.siteKey('notebook');
  // why nothing can run here (file://, no memory64), or null
  const unavailable = () => P.unavailable();

  // ---- small DOM helpers

  function h(tag, attrs, ...kids) {
    const el = document.createElement(tag);
    if (attrs) for (const k in attrs) {
      const v = attrs[k];
      if (v == null || v === false) continue;
      if (k === 'class') el.className = v;
      else if (k === 'text') el.textContent = v;
      else if (k.startsWith('on')) el.addEventListener(k.slice(2), v);
      else el.setAttribute(k, v === true ? '' : v);
    }
    for (const c of kids) if (c != null) el.appendChild(typeof c === 'string' ? document.createTextNode(c) : c);
    return el;
  }
  const ICONS = {
    run: ['M8 5.5v13a1 1 0 0 0 1.53.85l10.4-6.5a1 1 0 0 0 0-1.7L9.53 4.65A1 1 0 0 0 8 5.5z', true],
    up: ['M6 15l6-6 6 6'],
    down: ['M6 9l6 6 6-6'],
    code: ['M9 7l-5 5 5 5M15 7l5 5-5 5'],
    text: ['M5 6h14M5 11h14M5 16h9'],
    del: ['M4 7h16M10 11v6M14 11v6M6 7l1 13h10l1-13M9 7V4h6v3'],
  };
  function icon(name) {
    const [d, filled] = ICONS[name];
    const s = document.createElementNS(SVG_NS, 'svg');
    s.setAttribute('viewBox', '0 0 24 24');
    s.setAttribute('aria-hidden', 'true');
    if (filled) s.setAttribute('fill', 'currentColor');
    else {
      s.setAttribute('fill', 'none');
      s.setAttribute('stroke', 'currentColor');
      s.setAttribute('stroke-width', '2');
      s.setAttribute('stroke-linecap', 'round');
      s.setAttribute('stroke-linejoin', 'round');
    }
    const p = document.createElementNS(SVG_NS, 'path');
    p.setAttribute('d', d);
    s.appendChild(p);
    return s;
  }
  const clip = (s, n) => (s.length > n ? s.slice(0, n - 1) + '…' : s);
  const fmtNum = n => n.toLocaleString('en-US');
  const isTouch = !!P.touch;
  // soon: at the next animation frame, or, while the page is hidden and
  // gets none, as a task (page.js does the same for the Console)
  const later = f => (document.hidden ? setTimeout(f, 0) : requestAnimationFrame(f));

  // ---- the notebook model

  // cell: {id, type: 'code'|'markdown', source, count, outputs, status,
  //        stale, ranSource, editing, collapsed}; outputs are the saved
  //        kinds ({k:'stream'|'value'|'display'|'error'|'note', ...}),
  //        with their DOM nodes in the views map
  const nb = { title: 'Untitled', created: null, modified: null, pharo: null, cells: [] };
  const views = new Map();              // cell id -> view
  let selectedId = null;
  const trash = [];
  let outSeq = 0;

  const byId = id => nb.cells.find(c => c.id === id);
  const indexOf = cell => nb.cells.indexOf(cell);

  function newCell(type, source) {
    return { id: L.newId(), type, source: source || '', count: null, outputs: [], status: 'idle',
             stale: false, ranSource: null, editing: type === 'markdown' && !source, collapsed: false,
             sources: [], rt: null };
  }

  // ---- the kernel

  let kernel = null, kernelState = 'off', kernelUsed = false;
  const uploads = new Map();            // name -> Uint8Array, written into every new kernel

  function ensureKernel() {
    if (kernel) return kernel;
    if (unavailable() || !K) return null;
    kernel = K.create({
      createWorker: () => new Worker(P.workerUrl),
      getModule: () => P.wasmModule(),  // the Console's: compiled once
      init: { manifestUrl: 'manifest.json' + P.Q, build: P.BUILD, persist: true, gitProxy: P.gitProxy() },
      files: () => [...uploads].map(([n, d]) => ({ path: HOME + n, data: d })),
      watchdogMs: 3000,
      stopOnError: true,
      schedule: later,
      onCell, onState, onLog,
    });
    return kernel;
  }
  function startKernel() {
    const k = ensureKernel();
    if (!k) return;
    k.start().catch(e => {
      // the first line: a crash's stack is in the kernel messages
      const why = String((e && (e.reason || e.message)) || e).split('\n')[0];
      showNotice('kernel', 'The notebook kernel is unavailable: ' + clip(why, 400));
    });
  }

  const STATES = {
    off: ['off', 'kernel not started'], starting: ['loading', 'starting kernel'], idle: ['ready', 'idle'],
    busy: ['running', 'running'], sleeping: ['sleeping', 'sleeping'], input: ['waiting', 'waiting for input'],
    dead: ['exited', 'kernel stopped'], unavailable: ['error', 'unavailable'],
  };
  function setStatus(key, text) {
    statusEl.dataset.state = key;
    if (statusText.textContent !== text) statusText.textContent = text;
  }
  function onState(state, info) {
    kernelState = state;
    const [key, label] = STATES[state] || ['loading', state];
    let text = label;
    if ((state === 'busy' || state === 'sleeping' || state === 'input') && info && info.queued)
      text += ' · ' + info.queued + ' queued';
    if (state === 'dead' && info && info.reason) text = clip(String(info.reason), 60);
    setStatus(key, text);
    // (the page may have learnt meanwhile that no VM runs here: no memory64)
    if (state === 'unavailable' && unavailable()) lockOut();
    else if (state === 'unavailable')
      showNotice('kernel', 'The notebook kernel is unavailable: ' + ((info && info.reason) || 'it could not be loaded') +
                 (P.HTTP_HINT && !/HTTP/.test((info && info.reason) || '') ? ' ' + P.HTTP_HINT : ''));
    if (state === 'idle' && kernel && kernel.info) showInfo(kernel.info);
    updateToolbar();
  }
  // the kernel's Pharo, and its image: the one saved in this browser, or
  // the site's (ready.source 'saved' or 'download')
  const SOURCES = { saved: 'The image saved in this browser', download: 'The image of this site' };
  function showInfo(info) {
    const el = $('nb-info');
    el.textContent = 'Pharo ' + info.version + ' · wasm64';
    el.title = [SOURCES[info.source], info.image].filter(Boolean).join(': ') + ' (read-only: the notebook never saves it)';
    nb.pharo = String(info.version);
  }
  function onLog(kind, text) {
    log(kind, text);
    if (kind === 'kernel') announce(text.replace(/^./, c => c.toUpperCase()));
  }

  // ---- cell events from the kernel (batched per frame)

  const dirty = new Set();
  let frame = 0;
  function queueOp(cell, op) {
    (cell.ops || (cell.ops = [])).push(op);
    dirty.add(cell);
    if (!frame) frame = later(flush);
  }
  function flush() {
    frame = 0;
    for (const cell of dirty) flushCell(cell);
    dirty.clear();
  }
  // a frame asked for before the page was hidden never comes
  document.addEventListener('visibilitychange', () => { if (document.hidden && frame) flush(); });
  function flushCell(cell) {
    const ops = cell.ops;
    if (!ops || !ops.length) return;
    cell.ops = [];
    dirty.delete(cell);
    if (!views.has(cell.id)) return;
    for (let i = 0; i < ops.length; i++) {
      const op = ops[i];
      if (op.type === 'stream') {
        let text = op.text;
        while (i + 1 < ops.length && ops[i + 1].type === 'stream' && ops[i + 1].name === op.name &&
               text.length < STREAM_TAIL) text += ops[++i].text;
        appendStream(cell, op.name, text);
      } else if (op.type === 'display') addDisplay(cell, op);
      else if (op.type === 'clear') clearOutputs(cell);
      else if (op.type === 'error') addOutput(cell, { k: 'error', error: op.error, forked: true });
    }
  }

  // the cell whose run the kernel started last, until its done: a
  // cell with its id that is another object (a notebook loaded, a
  // deletion undone) gets none of that run's events
  let startedCell = null;
  function onCell(id, ev) {
    const cell = byId(id);
    const started = startedCell && startedCell.id === id ? startedCell : null;
    if (ev.type === 'start') startedCell = cell;
    else if (ev.type === 'done' && started) startedCell = null;
    if (ev.type === 'late') { late(cell, ev); return; }
    if (!cell) return;                  // deleted meanwhile
    if (started && started !== cell && ev.type !== 'queued' && ev.type !== 'cancelled' && ev.type !== 'start') return;
    switch (ev.type) {
    case 'queued':
      if (cell.status !== 'running' && cell.status !== 'waiting') cell.status = 'queued';
      chrome(cell);
      break;
    case 'start':
      flushCell(cell);
      cell.ops = [];
      clearOutputs(cell);
      clearHighlight(cell);
      cell.status = 'running';
      cell.runCount = ev.count;
      cell.ranSource = cell.sources.length ? cell.sources.shift() : cell.source;
      cell.stale = false;
      kernelUsed = true;
      chrome(cell);
      break;
    case 'stream': case 'display': case 'clear': case 'error':
      // (error: a process that the cell forked failed; the cell goes on)
      queueOp(cell, ev);
      break;
    case 'input':
      flushCell(cell);
      showWaiting(cell, ev.waiting);
      break;
    case 'done':
      flushCell(cell);
      finish(cell, ev);
      break;
    case 'cancelled':
      cell.sources.shift();
      // a cell also running (or just ended) keeps its status: only its
      // extra run is dropped
      if (cell.status === 'queued') {
        if (ev.reason === 'removed') cell.status = cell.prevStatus || 'idle';
        else {
          cell.status = 'cancelled';
          setNote(cell, CANCEL[ev.reason] || 'Not run (' + ev.reason + ').', true);
        }
      }
      chrome(cell);
      break;
    }
    updateToolbar();
  }

  // What a process forked by a cell writes or shows, or how it failed
  // ({kind: 'stream'|'display'|'clear'|'error', count, ...}): it goes to the
  // cell whose run of that count in this kernel forked it, while the cell
  // still shows that run; else to the kernel messages.
  function late(cell, ev) {
    const op = Object.assign({}, ev, { type: ev.kind });
    if (cell && cell.runCount != null && (ev.count == null || ev.count === cell.runCount) && views.has(cell.id)) {
      queueOp(cell, op);
      return;
    }
    const where = ev.count != null ? 'In[' + ev.count + ']' : 'a background process';
    if (op.type === 'stream') log(op.name || 'stdout', op.text);
    else if (op.type === 'error') log('kernel', where + ': ' + ((op.error && op.error.text) || 'an error'));
    else if (op.type === 'display') log('kernel', where + ' showed ' + clip(String(op.mime), 60) + ', not shown here');
  }

  const CANCEL = {
    stopped: 'Not run: stopped.',
    'previous cell failed': 'Not run: a previous cell failed.',
    restart: 'Not run: the kernel was restarted.',
    'kernel exited': 'Not run: the kernel exited.',
    'kernel crashed': 'Not run: the kernel crashed.',
    'kernel unavailable': 'Not run: the kernel is unavailable.',
    disposed: 'Not run.',
  };

  function finish(cell, ev) {
    showWaiting(cell, false);
    if (ev.count != null) cell.count = ev.count;
    else if (cell.runCount != null) cell.count = cell.runCount;
    const n = indexOf(cell) + 1;
    let said = null;
    switch (ev.status) {
    case 'ok':
      cell.status = 'ok';
      addValues(cell, ev.values, ev.truncated);
      said = 'Cell ' + n + ' finished' + (ev.values && ev.values.length ? ': ' + ev.values.join(' ') : '');
      break;
    case 'error':
    case 'syntax': {
      cell.status = 'error';
      const e = Object.assign({}, ev.error || { text: 'Error', kind: [], chain: [] });
      if (ev.status === 'syntax')
        e.text = 'Syntax error: ' + String(e.text).replace(/^(?:Syntax error|Error):\s*/, '') + ' — nothing was evaluated';
      addOutput(cell, { k: 'error', error: e });
      if (e.line) highlightLine(cell, e.line);
      said = 'Cell ' + n + ' failed: ' + e.text;
      break;
    }
    case 'interrupted':
      cell.status = 'interrupted';
      addOutput(cell, { k: 'note', text: 'Interrupted.' });
      said = 'Cell ' + n + ' interrupted';
      break;
    case 'terminated':                  // as CHICKEN's reset: not a failure
      cell.status = 'ok';
      addOutput(cell, { k: 'note', text: 'The cell\'s process terminated itself.' });
      said = 'Cell ' + n + ' finished';
      break;
    default: {                          // killed, exited, crashed
      cell.status = 'error';
      let msg;
      if (ev.status === 'exited') msg = 'Kernel exited (code ' + ev.code + '). Its state was lost; the next run starts a fresh kernel.';
      else if (ev.status === 'crashed') msg = 'Kernel crashed: ' + clip(String(ev.message || 'unknown error'), 400);
      else if (ev.reason === 'unresponsive') msg = 'The kernel did not respond to Stop and was restarted. Its state was lost.';
      else msg = 'The kernel was restarted while this cell ran.';
      addOutput(cell, { k: 'note', text: msg, bad: true });
      markStale(cell);
      said = 'Cell ' + n + ': ' + msg;
    }
    }
    chrome(cell);
    if (said) announce(clip(said, 120));
    scheduleSave();
  }

  // ---- outputs

  function clearOutputs(cell) {
    cell.outputs = [];
    cell.rt = null;
    const v = views.get(cell.id);
    if (v) v.out.textContent = '';
  }
  function addOutput(cell, o, at) {
    const v = views.get(cell.id);
    if (at != null) cell.outputs.splice(at, 0, o); else cell.outputs.push(o);
    if (!v) return o;
    const el = renderOutput(cell, o);
    els.set(o, el);
    if (at != null && at < cell.outputs.length - 1) {
      const next = els.get(cell.outputs[at + 1]);
      v.out.insertBefore(el, next || null);
    } else v.out.appendChild(el);
    return o;
  }
  const els = new WeakMap();            // output -> element
  function removeOutput(cell, o) {
    const i = cell.outputs.indexOf(o);
    if (i >= 0) cell.outputs.splice(i, 1);
    const el = els.get(o);
    if (el) el.remove();
  }
  function setNote(cell, text, transient) {
    for (const o of cell.outputs.filter(o => o.k === 'note' && o.transient)) removeOutput(cell, o);
    if (text) addOutput(cell, { k: 'note', text, transient });
  }

  function renderOutput(cell, o) {
    switch (o.k) {
    case 'stream': {
      const pre = h('pre', { class: 'nb-stream ' + o.name });
      const t = document.createTextNode(o.text);
      pre.appendChild(t);
      o._t = t;
      return pre;
    }
    case 'value': return renderValue(o.text);
    case 'note': return h('p', { class: 'nb-note' + (o.bad ? ' bad' : ''), text: o.text, hidden: o.text ? null : '' });
    case 'error': return renderError(cell, o.error, o.forked);
    case 'display': return renderDisplay(cell, o);
    }
    return h('p', { class: 'nb-note', text: 'unsupported output' });
  }

  // While a cell runs, its outputs are its first STREAM_HEAD characters
  // of streams and OUT_HEAD outputs, a note, and then its last
  // STREAM_TAIL characters and OUT_TAIL outputs: a flood of output, or
  // of shows, would swamp the page.  (Values and errors come after.)
  function runState(cell) {
    return cell.rt || (cell.rt = { head: 0, headN: 0, tail: 0, tailN: 0, omitted: 0, omittedN: 0, note: null });
  }
  const continues = (cell, name) => {
    const last = cell.outputs[cell.outputs.length - 1];
    return !!last && last.k === 'stream' && last.name === name;
  };
  function appendRaw(cell, name, text) {
    const last = cell.outputs[cell.outputs.length - 1];
    if (continues(cell, name)) {
      last.text += text;
      if (last._t) last._t.appendData(text);
      return;
    }
    addOutput(cell, { k: 'stream', name, text });
  }
  function appendStream(cell, name, text) {
    const rt = runState(cell);
    if (!rt.note) {
      const fits = continues(cell, name) || rt.headN < OUT_HEAD;
      const room = fits ? Math.min(text.length, STREAM_HEAD - rt.head) : 0;
      if (room > 0) {
        if (!continues(cell, name)) rt.headN++;
        rt.head += room;
        appendRaw(cell, name, room < text.length ? text.slice(0, room) : text);
      }
      if (room === text.length) return;
      text = text.slice(room);
      openNote(cell, rt);
    }
    if (text.length > STREAM_TAIL) {    // never more than the tail in the page
      rt.omitted += text.length - STREAM_TAIL;
      text = text.slice(-STREAM_TAIL);
    }
    if (!continues(cell, name)) rt.tailN++;
    appendRaw(cell, name, text);
    rt.tail += text.length;
    trimTail(cell, rt);
  }
  function openNote(cell, rt) {
    rt.note = addOutput(cell, { k: 'note', text: '' });      // hidden while empty
  }
  // a display (or the note of one too large, which has an id, maybe null)
  const shown = o => o.k === 'display' || o.k === 'note' && o.id !== undefined;
  // drops what is over the tail's budgets, from the start of the tail
  function trimTail(cell, rt) {
    let excess = rt.tail - STREAM_TAIL;
    const outs = cell.outputs;
    const i = excess > 0 || rt.tailN > OUT_TAIL ? outs.indexOf(rt.note) : -1;
    while (i >= 0 && (rt.tailN > OUT_TAIL || excess > 0)) {
      // too many outputs: the first one goes; too many characters: the
      // first stream, or its start
      const count = rt.tailN > OUT_TAIL;
      let j = i + 1;
      while (j < outs.length && !(outs[j].k === 'stream' || count && shown(outs[j]))) j++;
      const o = outs[j];
      if (!o) break;
      if (o.k === 'stream' && !count && o.text.length > excess) {
        o.text = o.text.slice(excess);
        if (o._t) o._t.deleteData(0, excess);
        rt.tail -= excess; rt.omitted += excess;
        break;
      }
      if (o.k === 'stream') { rt.tail -= o.text.length; rt.omitted += o.text.length; excess -= o.text.length; }
      else rt.omittedN++;
      rt.tailN--;
      removeOutput(cell, o);
    }
    const parts = [];
    if (rt.omitted) parts.push(fmtNum(rt.omitted) + ' characters');
    if (rt.omittedN) parts.push(fmtNum(rt.omittedN) + (rt.omittedN === 1 ? ' output' : ' outputs'));
    const text = parts.length ? '… ' + parts.join(' and ') + ' omitted …' : '';
    if (text === rt.note.text) return;
    rt.note.text = text;
    const el = els.get(rt.note);
    if (el) { el.textContent = text; el.hidden = !text; }
  }

  // a printString (the kernel cuts it at 64 K characters, and says so)
  function renderValue(text) {
    let t = String(text), extra = null;
    if (t.length > VALUE_MAX) {
      extra = h('p', { class: 'nb-note', text: '… value clipped: ' + fmtNum(t.length - VALUE_MAX) + ' more characters' });
      t = t.slice(0, VALUE_MAX);
    }
    const pre = h('pre', { class: 'nb-value', tabindex: '-1', text: t });
    const lines = P.countLines(t) + 1;
    if (lines <= VALUE_LINES && !extra) return pre;
    const box = h('div', { class: 'nb-valuebox' }, pre);
    if (lines > VALUE_LINES) {
      pre.classList.add('clamped');
      const more = h('button', { class: 'btn nb-more', type: 'button', 'aria-expanded': 'false', text: 'Show all ' + fmtNum(lines) + ' lines' });
      more.addEventListener('click', () => {
        const open = pre.classList.toggle('clamped');
        more.setAttribute('aria-expanded', String(!open));
        more.textContent = open ? 'Show all ' + fmtNum(lines) + ' lines' : 'Show less';
      });
      box.appendChild(more);
    }
    if (extra) box.appendChild(extra);
    return box;
  }
  // values: null when the cell has no value to print (no statements, nil,
  // or a display, which came as one)
  function addValues(cell, values, truncated) {
    if (values == null) return;
    for (const v of values) if (v != null) addOutput(cell, { k: 'value', text: String(v) });
    if (truncated) addOutput(cell, { k: 'note', text: '… the printString was cut at ' + fmtNum(VALUE_MAX) + ' characters' });
  }

  // ERR {text, kind, location, form, line, column, chain: [{where, proc,
  // form}]}, the chain outermost first; FORKED: of a process the cell forked
  function renderError(cell, e, forked) {
    const box = h('div', { class: 'nb-error' });
    const syntax = /^Syntax error:/.test(e.text);
    const msg = h('p', { class: 'nb-error-msg' },
                  h('span', { class: 'tag', text: syntax ? 'Syntax error' : forked ? 'Error in a forked process' : 'Error' }),
                  String(e.text).replace(/^(?:Syntax error|Error):\s*/, '').replace(/\s+$/, ''));
    box.appendChild(msg);
    const chain = Array.isArray(e.chain) ? e.chain : [];
    if (chain.length) {
      const det = h('details', { class: 'nb-trace' }, h('summary', { text: 'Call history (' + chain.length + ')' }));
      const ol = h('ol');
      chain.forEach((f, i) => {
        const last = i === chain.length - 1;
        // In[R]:LINE, a frame of the cell's run R: its line in the cell
        const m = /^In\[(\d+)\]:(\d+)$/.exec(f.where);
        let where;
        if (m && Number(m[1]) === cell.count) {
          where = h('button', { class: 'where', type: 'button', title: 'Show line ' + m[2], text: f.where });
          where.addEventListener('click', () => selectLine(cell, Number(m[2])));
        } else where = h('span', { class: 'where', text: f.where });
        ol.appendChild(h('li', { class: last ? 'last' : null }, where,
                         h('span', { class: 'form', text: (f.form || f.proc || '') + (last ? '   <--' : '') })));
      });
      det.appendChild(ol);
      box.appendChild(det);
    }
    return box;
  }

  function renderDisplay(cell, o) {
    const n = indexOf(cell) + 1;
    const div = h('div', { class: 'nb-rich', 'data-mime': o.mime });
    const data = String(o.data);
    if (data.length > DISPLAY_MAX) return h('p', { class: 'nb-note', text: tooLarge(o.mime, data.length) });
    const prefix = 'o' + (++outSeq) + '-';
    try {
      switch (o.mime) {
      case 'text/plain': div.appendChild(h('pre', { text: data })); break;
      case 'text/html': div.appendChild(L.sanitizeMarkup(data, 'html', prefix)); break;
      case 'image/svg+xml': div.appendChild(L.sanitizeMarkup(data, 'svg', prefix)); break;
      case 'text/markdown': div.appendChild(L.renderMarkdown(data, document, prefix)); break;
      case 'image/png': case 'image/jpeg': case 'image/gif': case 'image/webp': {
        const b64 = data.replace(/\s+/g, '');
        if (!/^[A-Za-z0-9+/]*={0,2}$/.test(b64)) throw new Error('the image data is not base64');
        div.appendChild(h('img', { src: 'data:' + o.mime + ';base64,' + b64, alt: 'Image output of cell ' + n }));
        break;
      }
      default:
        return h('p', { class: 'nb-note', text: 'unsupported output type ' + clip(String(o.mime), 60) });
      }
    } catch (e) {
      return h('p', { class: 'nb-note bad', text: 'Could not show the ' + o.mime + ' output: ' + ((e && e.message) || e) });
    }
    return div;
  }
  // (a size of 0 is an output with nothing in it, as a Form 0 pixels wide)
  const tooLarge = (mime, n) => n > 0 ? 'Output too large to show (' + (n / 1048576).toFixed(1) + ' MB of ' + mime + ').'
    : 'An empty output (' + mime + ').';
  // show: and show:id: (the same id in the same cell replaces it)
  function addDisplay(cell, ev) {
    const id = ev.id == null ? null : String(ev.id);
    // data too large for the kernel to send: a note (which is saved)
    const o = ev.size != null ? { k: 'note', text: tooLarge(ev.mime, ev.size), id }
      : { k: 'display', mime: ev.mime, data: ev.data, id };
    if (id != null) {
      const prev = cell.outputs.find(x => (x.k === 'display' || x.k === 'note') && x.id === id);
      if (prev) {
        const i = cell.outputs.indexOf(prev);
        removeOutput(cell, prev);
        addOutput(cell, o, i);
        return;
      }
    }
    const rt = runState(cell);
    if (!rt.note) {
      if (rt.headN < OUT_HEAD) { rt.headN++; addOutput(cell, o); return; }
      openNote(cell, rt);
    }
    rt.tailN++;
    addOutput(cell, o);
    trimTail(cell, rt);
  }

  // ---- a cell that waits for input

  // The kernel never offers its stdin to cells: one that reads it (Stdio
  // stdin) waits until it is stopped.
  function showWaiting(cell, on) {
    const v = views.get(cell.id);
    if (!v) return;
    if (cell.status === 'running' || cell.status === 'waiting') cell.status = on ? 'waiting' : 'running';
    chrome(cell);
    if (!on) {
      if (v.wait) {
        // the cell, not <body>, keeps the focus of its Stop
        const had = v.wait.contains(document.activeElement);
        v.wait.hidden = true;
        if (had) select(cell.id, { focus: 'cell' });
      }
      return;
    }
    if (!v.wait) {
      const b = h('button', { class: 'btn', type: 'button', text: 'Stop' });
      b.addEventListener('click', stop);
      v.wait = h('div', { class: 'nb-stdin', hidden: true },
                 h('span', { class: 'nb-wait-text', text: 'This cell waits for input on stdin, which the notebook does not provide.' }), b);
      v.body.appendChild(v.wait);
    }
    v.wait.hidden = false;
    announce('Cell ' + (indexOf(cell) + 1) + ' waits for input on stdin, which the notebook does not provide');
  }

  // ---- cell views

  function makeView(cell) {
    const li = h('li', { class: 'nb-cell', 'data-id': cell.id, 'data-type': cell.type, tabindex: '-1' });
    const run = h('button', { class: 'nb-run btn ghost', type: 'button' }, icon('run'));
    const count = h('span', { class: 'nb-count', 'aria-hidden': 'true' });
    const src = h('textarea', { class: 'nb-src', rows: '1', spellcheck: 'false', autocapitalize: 'off', autocomplete: 'off',
                                autocorrect: 'off', wrap: 'soft' });
    src.value = cell.source;
    // the line of an error, marked behind the editor (the overlay mirrors its text)
    const hl = h('div', { class: 'nb-hl', 'aria-hidden': 'true' });
    const bal = h('span', { class: 'nb-bal', 'aria-hidden': 'true', hidden: true });
    const editor = h('div', { class: 'nb-editor' }, hl, src, bal);
    const md = h('div', { class: 'nb-md' });
    const out = h('div', { class: 'nb-out', role: 'group' });
    const showOut = h('button', { class: 'btn ghost nb-hidden-out', type: 'button', hidden: true, text: 'Output hidden — show it' });
    const body = h('div', { class: 'nb-body' }, editor, md, out, showOut);
    const up = h('button', { class: 'btn ghost', type: 'button', 'data-act': 'up' }, icon('up'));
    const down = h('button', { class: 'btn ghost', type: 'button', 'data-act': 'down' }, icon('down'));
    const kind = h('button', { class: 'btn ghost', type: 'button', 'data-act': 'type' }, icon(cell.type === 'code' ? 'text' : 'code'));
    const del = h('button', { class: 'btn ghost del', type: 'button', 'data-act': 'del' }, icon('del'));
    const tools = h('div', { class: 'nb-tools', role: 'toolbar' }, up, down, kind, del);
    // the tools come before the editor, which keeps Tab for indenting
    li.append(h('div', { class: 'nb-gutter' }, run, count), tools, body);
    const v = { li, run, count, src, hl, bal, editor, md, out, showOut, body, tools, up, down, kind, del, wait: null };
    views.set(cell.id, v);

    run.addEventListener('click', () => { select(cell.id); runOne(cell, cell.type === 'markdown' ? 'cell' : null); });
    up.addEventListener('click', () => moveCell(cell, -1, 'button'));
    down.addEventListener('click', () => moveCell(cell, 1, 'button'));
    kind.addEventListener('click', () => setType(cell, cell.type === 'code' ? 'markdown' : 'code'));
    del.addEventListener('click', () => deleteCell(cell));
    showOut.addEventListener('click', () => toggleOutput(cell));
    src.addEventListener('input', () => onEdit(cell));
    md.addEventListener('dblclick', () => editCell(cell));
    for (const o of cell.outputs) { const el = renderOutput(cell, o); els.set(o, el); out.appendChild(el); }
    if (cell.type === 'markdown') renderMd(cell);
    chrome(cell);
    return li;
  }

  // heading ids of the markdown cells, unique in the notebook
  const mdIds = new Set(), mdIdsOf = new Map();
  function releaseMdIds(cell) {
    for (const id of mdIdsOf.get(cell.id) || []) mdIds.delete(id);
    mdIdsOf.delete(cell.id);
  }
  function renderMd(cell) {
    const v = views.get(cell.id);
    if (!v) return;
    releaseMdIds(cell);
    const frag = L.renderMarkdown(cell.source, document, 'nb-h-', { ids: mdIds });
    mdIdsOf.set(cell.id, Array.from(frag.querySelectorAll('[id]'), el => el.id));
    v.md.replaceChildren(frag);
    v.md.classList.toggle('empty', !cell.source.trim());
  }

  const STATUS_WORDS = {
    idle: null, queued: 'queued', running: 'running', waiting: 'waiting for input', ok: 'succeeded',
    error: 'failed', interrupted: 'interrupted', cancelled: 'not run',
  };
  // count, data-* attributes and labels
  function chrome(cell) {
    const v = views.get(cell.id);
    if (!v) return;
    const n = indexOf(cell) + 1, code = cell.type === 'code';
    const li = v.li;
    li.dataset.type = cell.type;
    li.dataset.status = cell.status;
    const edited = code && cell.count != null && cell.ranSource != null && cell.ranSource !== cell.source;
    if (cell.stale) li.dataset.stale = 'true'; else delete li.dataset.stale;
    if (edited) li.dataset.edited = 'true'; else delete li.dataset.edited;
    li.classList.toggle('is-editing', !!cell.editing);
    if (cell.status === 'running' || cell.status === 'waiting') li.setAttribute('aria-busy', 'true');
    else li.removeAttribute('aria-busy');
    const c = cell.status === 'running' || cell.status === 'waiting' ? '[*]' : cell.status === 'queued' ? '[…]'
      : cell.count != null ? '[' + cell.count + ']' : '[ ]';
    if (v.count.textContent !== c) v.count.textContent = c;
    let label = (code ? 'Code' : 'Text') + ' cell ' + n;
    if (code && cell.count != null) label += ', run [' + cell.count + ']';
    const w = code && STATUS_WORDS[cell.status];
    if (w) label += ', ' + w;
    if (cell.stale) label += ', from an earlier session';
    if (edited) label += ', edited since it ran';
    li.setAttribute('aria-label', label);
    v.run.setAttribute('aria-label', (code ? 'Run cell ' : 'Render cell ') + n);
    v.run.title = code ? 'Run (Shift+Enter)' : 'Render (Shift+Enter)';
    v.src.setAttribute('aria-label', code ? 'Cell ' + n + ' source (' + S.LANGUAGE + ')' : 'Cell ' + n + ' text (Markdown)');
    v.src.placeholder = code ? S.LANGUAGE + ' code' : 'Markdown text';
    v.out.setAttribute('aria-label', 'Output of cell ' + n);
    v.tools.setAttribute('aria-label', 'Cell ' + n + ' actions');
    v.up.setAttribute('aria-label', 'Move cell ' + n + ' up');
    v.up.title = 'Move up (Alt+↑)';
    v.down.setAttribute('aria-label', 'Move cell ' + n + ' down');
    v.down.title = 'Move down (Alt+↓)';
    v.kind.setAttribute('aria-label', code ? 'Make cell ' + n + ' a text cell' : 'Make cell ' + n + ' a code cell');
    v.kind.title = code ? 'Make it text (m)' : 'Make it code (y)';
    v.del.setAttribute('aria-label', 'Delete cell ' + n);
    v.del.title = 'Delete (d, d)';
    v.up.disabled = n === 1;
    v.down.disabled = n === nb.cells.length;
    v.out.hidden = !!cell.collapsed;
    v.showOut.hidden = !(cell.collapsed && cell.outputs.length);
  }
  function chromeAll() { for (const c of nb.cells) chrome(c); }

  // editors grow with their content
  function autosize(v) {
    const ta = v.src;
    if (!ta.isConnected || panel.hidden) return;
    ta.style.height = 'auto';
    ta.style.height = ta.scrollHeight + 'px';
  }
  // all at once: the heights are written, read and written again in
  // three passes, so that the page is laid out once, not once per cell
  // (5000 cells took half a minute)
  let sizeFrame = 0;
  function resizeAll() {
    if (sizeFrame) return;
    sizeFrame = requestAnimationFrame(() => {
      sizeFrame = 0;
      if (panel.hidden) return;
      const tas = [...views.values()].map(v => v.src).filter(ta => ta.isConnected);
      for (const ta of tas) ta.style.height = 'auto';
      const hs = tas.map(ta => ta.scrollHeight);
      tas.forEach((ta, i) => { ta.style.height = hs[i] + 'px'; });
    });
  }
  addEventListener('resize', resizeAll);

  function onEdit(cell) {
    const v = views.get(cell.id);
    cell.source = v.src.value;
    autosize(v);
    clearHighlight(cell);
    updateBalance(cell);
    chrome(cell);
    scheduleSave();
  }
  // what is unclosed or extra, from NotebookSt ('' when balanced)
  function updateBalance(cell) {
    const v = views.get(cell.id);
    if (!v) return;
    if (cell.type !== 'code') { v.bal.hidden = true; return; }
    const t = S.balanceText(cell.source);
    v.bal.textContent = t;
    v.bal.hidden = !t;
  }

  // a highlighted line behind the editor (the overlay mirrors its text)
  function highlightLine(cell, line) {
    const v = views.get(cell.id);
    if (!v) return;
    const lines = cell.source.split('\n');
    if (line < 1 || line > lines.length) return;
    const before = lines.slice(0, line - 1).join('\n') + (line > 1 ? '\n' : '');
    const after = (line < lines.length ? '\n' : '') + lines.slice(line).join('\n');
    v.hl.replaceChildren(document.createTextNode(before), h('mark', { text: lines[line - 1] || ' ' }), document.createTextNode(after));
  }
  function clearHighlight(cell) {
    const v = views.get(cell.id);
    if (v && v.hl.firstChild) v.hl.textContent = '';
  }
  function selectLine(cell, line) {
    const v = views.get(cell.id);
    if (!v) return;
    const lines = cell.source.split('\n');
    if (line < 1 || line > lines.length) return;
    const start = lines.slice(0, line - 1).reduce((a, l) => a + l.length + 1, 0);
    highlightLine(cell, line);
    v.src.focus();
    v.src.setSelectionRange(start, start + lines[line - 1].length);
  }

  // ---- structure

  function insertCell(at, type, source, quiet) {
    const cell = newCell(type, source);
    at = Math.max(0, Math.min(nb.cells.length, at));
    nb.cells.splice(at, 0, cell);
    const li = makeView(cell);
    const next = nb.cells[at + 1];
    cellsEl.insertBefore(li, next ? views.get(next.id).li : null);
    autosize(views.get(cell.id));
    if (!quiet) { chromeAll(); scheduleSave(); }
    return cell;
  }

  function deleteCell(cell) {
    const i = indexOf(cell);
    if (i < 0) return;
    // a running cell is stopped, as by Stop
    if (kernel) { if (kernel.current === cell.id) kernel.stop(); else kernel.cancel(cell.id); }
    flushCell(cell);
    const snap = Object.assign({}, cell, { outputs: cell.outputs.map(stripOutput), sources: [], ops: [], rt: null,
                                           status: cell.status === 'running' || cell.status === 'waiting' || cell.status === 'queued' ? 'idle' : cell.status });
    trash.push({ cell: snap, index: i });
    if (trash.length > TRASH_MAX) trash.shift();
    const hadFocus = panel.contains(document.activeElement) && views.get(cell.id).li.contains(document.activeElement);
    views.get(cell.id).li.remove();
    views.delete(cell.id);
    releaseMdIds(cell);
    nb.cells.splice(i, 1);
    if (!nb.cells.length) insertCell(0, 'code', '', true);
    const next = nb.cells[Math.min(i, nb.cells.length - 1)];
    chromeAll();
    select(next.id, { focus: hadFocus || selectedId === cell.id ? 'cell' : null });
    announce('Cell ' + (i + 1) + ' deleted');
    toast('Cell deleted.', 'Undo', undoDelete, 8000);
    scheduleSave();
    updateToolbar();
  }
  function undoDelete() {
    const t = trash.pop();
    if (!t) { announce('Nothing to undo'); return; }
    const cell = t.cell;
    if (byId(cell.id)) cell.id = L.newId();
    const at = Math.min(t.index, nb.cells.length);
    nb.cells.splice(at, 0, cell);
    const li = makeView(cell);
    const next = nb.cells[at + 1];
    cellsEl.insertBefore(li, next ? views.get(next.id).li : null);
    autosize(views.get(cell.id));
    updateBalance(cell);
    chromeAll();
    select(cell.id, { focus: 'cell' });
    hideToast();
    announce('Cell ' + (at + 1) + ' restored');
    scheduleSave();
  }

  function moveCell(cell, d, how) {
    const i = indexOf(cell), j = i + d;
    if (i < 0 || j < 0 || j >= nb.cells.length) return;
    const ae = document.activeElement;
    const sel = ae && ae.classList && ae.classList.contains('nb-src') ? [ae.selectionStart, ae.selectionEnd] : null;
    nb.cells.splice(i, 1);
    nb.cells.splice(j, 0, cell);
    const v = views.get(cell.id);
    const ref = nb.cells[j + 1];
    cellsEl.insertBefore(v.li, ref ? views.get(ref.id).li : null);
    chromeAll();
    if (sel) { ae.focus({ preventScroll: true }); ae.setSelectionRange(sel[0], sel[1]); }
    else if (how === 'button') {
      const b = d < 0 ? v.up : v.down;
      (b.disabled ? v.li : b).focus({ preventScroll: true });
    } else if (ae && v.li.contains(ae)) ae.focus({ preventScroll: true });
    v.li.scrollIntoView({ block: 'nearest' });
    announce('Cell moved to position ' + (j + 1));
    scheduleSave();
  }

  function setType(cell, type) {
    if (cell.type === type) return;
    if (kernel && cell.status === 'queued') kernel.cancel(cell.id);
    if (cell.status === 'running' || cell.status === 'waiting') return;
    const v = views.get(cell.id);
    const hadFocus = v && v.li.contains(document.activeElement);
    cell.type = type;
    releaseMdIds(cell);
    cell.outputs = [];
    cell.count = null;
    cell.status = 'idle';
    cell.stale = false;
    cell.ranSource = null;
    cell.editing = type === 'markdown' && !cell.source.trim();
    const li = makeView(cell);
    v.li.replaceWith(li);
    autosize(views.get(cell.id));
    updateBalance(cell);
    chromeAll();
    select(cell.id, { focus: hadFocus ? 'cell' : null });
    announce('Cell ' + (indexOf(cell) + 1) + ' is now a ' + (type === 'code' ? 'code' : 'text') + ' cell');
    scheduleSave();
  }

  function toggleOutput(cell) {
    cell.collapsed = !cell.collapsed;
    chrome(cell);
    announce(cell.collapsed ? 'Output hidden' : 'Output shown');
  }

  // ---- selection and modes

  // the cell left with Esc: Tab right after it leaves the cell (Tab in
  // its editor indents)
  let tabOut = null;

  function select(id, opt) {
    opt = opt || {};
    const prev = selectedId && views.get(selectedId);
    if (prev && selectedId !== id) { prev.li.classList.remove('is-selected'); prev.li.tabIndex = -1; }
    selectedId = id;
    const v = views.get(id);
    if (!v) return;
    v.li.classList.add('is-selected');
    v.li.tabIndex = 0;
    if (opt.focus === 'cell') { v.li.focus({ preventScroll: true }); v.li.scrollIntoView({ block: 'nearest' }); }
    else if (opt.focus === 'editor') editCell(byId(id), opt.caret);
  }
  function editCell(cell, caret) {
    const v = views.get(cell.id);
    if (!v) return;
    if (cell.type === 'markdown' && !cell.editing) { cell.editing = true; chrome(cell); }
    select(cell.id);
    autosize(v);
    v.src.focus({ preventScroll: true });
    if (caret === 'start') v.src.setSelectionRange(0, 0);
    else if (caret === 'end') v.src.setSelectionRange(v.src.value.length, v.src.value.length);
    v.editor.scrollIntoView({ block: 'nearest' });
  }
  function commandMode(cell) {
    if (cell.type === 'markdown' && cell.editing) { cell.editing = false; renderMd(cell); chrome(cell); }
    select(cell.id, { focus: 'cell' });
  }
  function selectedCell() { return byId(selectedId) || nb.cells[0]; }

  cellsEl.addEventListener('focusin', e => {
    if (tabOut && e.target !== (views.get(tabOut) || {}).li) tabOut = null;
    const li = e.target.closest && e.target.closest('.nb-cell');
    if (li && li.dataset.id !== selectedId) select(li.dataset.id);
  });
  cellsEl.addEventListener('mousedown', e => {
    const li = e.target.closest('.nb-cell');
    if (!li) return;
    if (li.dataset.id !== selectedId) select(li.dataset.id);
    // a click on the cell's chrome (not a control) gives command mode
    if (!e.target.closest('textarea, input, button, a, summary, .nb-out, .nb-md, .nb-stdin')) {
      e.preventDefault();
      li.focus({ preventScroll: true });
    }
  });

  // ---- running

  // A touch keyboard types curly quotes, which are no Smalltalk: straight
  // ones outside strings and comments (NotebookSt.straightenQuotes)
  function prepSource(cell) {
    if (isTouch) {
      const s = S.straightenQuotes(cell.source);
      if (s !== cell.source) {
        cell.source = s;
        const v = views.get(cell.id);
        if (v) { const a = v.src.selectionStart; v.src.value = s; v.src.setSelectionRange(a, a); }
      }
    }
    return cell.source;
  }
  // why nothing runs, on one line
  const unavailableText = () => String(unavailable()).replace(/\n/g, ' ');
  // nothing can run in this page (P.unavailable): the controls that would
  // are disabled, and the notice says why
  function lockOut() {
    for (const id of ['nb-run-all', 'nb-restart']) $(id).disabled = true;
    for (const act of ['restart-run', 'upload', 'stop-background']) {
      const b = menuItem(act);
      if (b) b.setAttribute('aria-disabled', 'true');
    }
    $('nb-stop').setAttribute('aria-disabled', 'true');
    setStatus('error', 'unavailable');
    showNotice('unavailable', unavailableText());
  }
  function unavailableNow() {
    if (unavailable()) {
      setStatus('error', 'unavailable');
      showNotice('unavailable', unavailableText());
      return true;
    }
    if (!K) { setStatus('error', 'kernel unavailable'); return true; }
    return false;
  }
  function runCells(list) {
    for (const c of list) if (c.type === 'markdown' && c.editing) { c.editing = false; renderMd(c); chrome(c); }
    const code = list.filter(c => c.type === 'code');
    if (!code.length || unavailableNow()) return;
    const k = ensureKernel();
    if (!k) return;
    hideNotice('restored');
    const jobs = code.map(c => {
      const source = prepSource(c);
      c.sources.push(source);
      if (c.status !== 'queued' && c.status !== 'running' && c.status !== 'waiting') c.prevStatus = c.status;
      return { cellId: c.id, source };
    });
    if (jobs.length === 1) k.run(jobs[0].cellId, jobs[0].source);
    else k.runMany(jobs);
    updateToolbar();
  }
  function runOne(cell, then) {
    runCells([cell]);
    if (then === 'cell') select(cell.id, { focus: 'cell' });
  }
  function runAll() {
    runCells(nb.cells.slice());
  }
  // a new VM: the counter starts at 1 again, and what ran before is stale
  async function restartKernel(andRun) {
    if (unavailableNow()) return;
    const k = ensureKernel();
    if (!k) return;
    hideNotice('image');
    markStale(null);
    announce('Restarting the kernel');
    try { await k.restart(); } catch (e) { return; }
    announce('Kernel restarted');
    if (andRun) runAll();
  }
  function stop() {
    if (!kernel) return;
    kernel.stop();
    announce('Stopping');
  }
  // the processes that cells forked and left running (not the cell running)
  async function stopBackground() {
    if (!kernel || kernelState === 'off' || kernelState === 'dead' || kernelState === 'unavailable') {
      toast('The kernel is not running: no background processes.');
      return;
    }
    const n = Number(await kernel.stopBackground()) || 0;
    toast('Stopped ' + fmtNum(n) + ' background process' + (n === 1 ? '' : 'es') + '.');
  }
  function markStale(except) {
    for (const c of nb.cells) {
      if (c === except || c.type !== 'code') continue;
      if (c.count != null || c.outputs.length) { c.stale = true; chrome(c); }
    }
  }

  function runKey(e, cell, fromEdit) {
    if (e.key !== 'Enter' || e.isComposing) return false;
    const mod = e.ctrlKey || e.metaKey;
    if (e.shiftKey && !mod && !e.altKey) {
      runCells([cell]);
      let i = indexOf(cell) + 1, created = false;
      if (i >= nb.cells.length) { insertCell(i, 'code', ''); created = true; }
      const next = nb.cells[i];
      if (next.type === 'code' && (fromEdit || created)) editCell(next, 'end');
      else select(next.id, { focus: 'cell' });
    } else if (mod && !e.shiftKey && !e.altKey) {
      runCells([cell]);
      if (cell.type === 'markdown' || !fromEdit) select(cell.id, { focus: 'cell' });
    } else if (e.altKey && !mod && !e.shiftKey) {
      runCells([cell]);
      const c = insertCell(indexOf(cell) + 1, 'code', '');
      editCell(c);
    } else return false;
    e.preventDefault();
    return true;
  }

  // the text editing helpers keep the browser's undo history when they can
  function insertText(ta, text) {
    let ok = false;
    try { ok = document.execCommand('insertText', false, text); } catch (e) { ok = false; }
    if (!ok) {
      ta.setRangeText(text, ta.selectionStart, ta.selectionEnd, 'end');
      ta.dispatchEvent(new Event('input'));
    }
  }
  function reindent(ta, f) {
    const v = ta.value, a = ta.selectionStart, b = ta.selectionEnd;
    const s = v.lastIndexOf('\n', a - 1) + 1;
    let e = v.indexOf('\n', b > a && v[b - 1] === '\n' ? b - 1 : b);
    if (e < 0) e = v.length;
    const lines = v.slice(s, e).split('\n');
    const out = lines.map(f);
    const text = out.join('\n');
    if (text === v.slice(s, e)) return;
    ta.setSelectionRange(s, e);
    insertText(ta, text);
    if (lines.length === 1 && a === b) {
      const pos = Math.max(s, a + (out[0].length - lines[0].length));
      ta.setSelectionRange(pos, pos);
    } else ta.setSelectionRange(s, s + text.length);
  }

  // Pharo indents with tabs (shown 4 wide): Tab inserts one, Shift+Tab
  // takes away a tab or up to 4 spaces, Enter keeps the indent of the line
  // and adds a tab after an unclosed ( [ {
  function editKey(e, cell) {
    const ta = e.target;
    if (runKey(e, cell, true)) return;
    const mod = e.ctrlKey || e.metaKey;
    const v = ta.value, a = ta.selectionStart, b = ta.selectionEnd;
    if (e.key === 'Escape') { e.preventDefault(); commandMode(cell); tabOut = cell.id; return; }
    if (e.key === 'Enter' && !e.shiftKey && !mod && !e.altKey && !e.isComposing && cell.type === 'code') {
      e.preventDefault();
      const ls = v.lastIndexOf('\n', a - 1) + 1;
      const line = v.slice(ls, a);
      const indent = /^[ \t]*/.exec(line)[0];
      insertText(ta, '\n' + indent + (S.balance(line).depth > 0 ? '\t' : ''));
      return;
    }
    if (e.key === 'Tab' && !mod && !e.altKey) {
      if (!v) return;                   // an empty cell: Tab moves focus
      e.preventDefault();
      if (e.shiftKey) reindent(ta, l => l.replace(/^(?:\t| {1,4})/, ''));
      else if (a !== b && v.slice(a, b).includes('\n')) reindent(ta, l => (l ? '\t' + l : l));
      else insertText(ta, '\t');
      return;
    }
    if ((e.key === 'ArrowUp' || e.key === 'ArrowDown') && e.altKey && !mod && !e.shiftKey) {
      e.preventDefault();
      moveCell(cell, e.key === 'ArrowUp' ? -1 : 1);
      return;
    }
    if (e.key === 'ArrowUp' && !e.altKey && !mod && !e.shiftKey && a === b && v.lastIndexOf('\n', a - 1) < 0) {
      const prev = nb.cells[indexOf(cell) - 1];
      if (prev) {
        e.preventDefault();
        if (cell.type === 'markdown' && cell.editing && cell.source.trim()) { cell.editing = false; renderMd(cell); chrome(cell); }
        if (prev.type === 'code' || prev.editing) editCell(prev, 'end'); else select(prev.id, { focus: 'cell' });
      }
      return;
    }
    if (e.key === 'ArrowDown' && !e.altKey && !mod && !e.shiftKey && a === b && v.indexOf('\n', a) < 0) {
      const next = nb.cells[indexOf(cell) + 1];
      if (next) {
        e.preventDefault();
        if (cell.type === 'markdown' && cell.editing && cell.source.trim()) { cell.editing = false; renderMd(cell); chrome(cell); }
        if (next.type === 'code' || next.editing) editCell(next, 'start'); else select(next.id, { focus: 'cell' });
      }
    }
  }

  let lastKey = '', lastKeyAt = 0;
  function commandKey(e, cell) {
    if (/^(Shift|Control|Alt|Meta)$/.test(e.key)) return;
    const out = tabOut === cell.id;
    tabOut = null;
    if (out && e.key === 'Tab' && !e.shiftKey && !e.ctrlKey && !e.metaKey && !e.altKey) {
      e.preventDefault();
      const next = nb.cells[indexOf(cell) + 1];
      if (next) select(next.id, { focus: 'cell' }); else $('nb-end-code').focus();
      return;
    }
    if (runKey(e, cell, false)) return;
    const mod = e.ctrlKey || e.metaKey;
    if (e.altKey && !mod && (e.key === 'ArrowUp' || e.key === 'ArrowDown')) {
      e.preventDefault();
      moveCell(cell, e.key === 'ArrowUp' ? -1 : 1);
      return;
    }
    if (mod || e.altKey) return;
    const k = e.key;
    const now = performance.now();
    const twice = k === lastKey && now - lastKeyAt < 800;
    lastKey = twice ? '' : k;
    lastKeyAt = now;
    const i = indexOf(cell);
    switch (k) {
    case 'ArrowUp': case 'k': if (i > 0) select(nb.cells[i - 1].id, { focus: 'cell' }); break;
    case 'ArrowDown': case 'j': if (i < nb.cells.length - 1) select(nb.cells[i + 1].id, { focus: 'cell' }); break;
    case 'Home': select(nb.cells[0].id, { focus: 'cell' }); break;
    case 'End': select(nb.cells[nb.cells.length - 1].id, { focus: 'cell' }); break;
    case 'Enter': editCell(cell, 'end'); break;
    case 'a': select(insertCell(i, 'code', '').id, { focus: 'cell' }); announce('Cell inserted above'); break;
    case 'b': select(insertCell(i + 1, 'code', '').id, { focus: 'cell' }); announce('Cell inserted below'); break;
    case 'd': if (twice) deleteCell(cell); break;
    case 'z': undoDelete(); break;
    case 'm': setType(cell, 'markdown'); break;
    case 'y': setType(cell, 'code'); break;
    case 'o': toggleOutput(cell); break;
    case 'i': if (twice) stop(); break;
    case '0':
      if (twice && (!kernelUsed || confirm('Restart the kernel? Its variables, classes and state are lost.'))) restartKernel();
      break;
    case '?': openShortcuts(); break;
    default: return;
    }
    e.preventDefault();
  }

  cellsEl.addEventListener('keydown', e => {
    const li = e.target.closest('.nb-cell');
    if (!li) return;
    const cell = byId(li.dataset.id);
    if (!cell) return;
    if (e.target.classList.contains('nb-src')) editKey(e, cell);
    else if (e.target === li) commandKey(e, cell);
    else if (e.key === 'Escape') { e.preventDefault(); commandMode(cell); tabOut = cell.id; }
  });

  // anywhere on the tab: Ctrl+C stops when nothing is selected, Ctrl+S
  // saves (page.js's keys act on the Console's tab only)
  document.addEventListener('keydown', e => {
    if (panel.hidden || document.querySelector('dialog[open]')) return;
    const k = e.key.toLowerCase();
    if (e.ctrlKey && !e.shiftKey && !e.altKey && !e.metaKey && k === 'c') {
      if (!kernel || !kernel.current) return;
      // a selection to copy: in a text field, else in the document
      const sel = getSelection(), t = e.target;
      const field = t instanceof HTMLTextAreaElement || t instanceof HTMLInputElement;
      if (field ? t.selectionStart !== t.selectionEnd : sel && !sel.isCollapsed) return;
      e.preventDefault();
      stop();
    } else if ((e.ctrlKey || e.metaKey) && !e.shiftKey && !e.altKey && k === 's') {
      e.preventDefault();
      saveNow();
      toast(saveFailed ? 'Could not save in this browser.' : 'Saved in this browser.');
    }
  });

  // ---- toolbar, menu, notices

  function updateToolbar() {
    const busy = !!(kernel && (kernel.current || kernel.queueLength));
    $('nb-stop').setAttribute('aria-disabled', String(!busy));
  }
  $('nb-run-all').addEventListener('click', runAll);
  $('nb-stop').addEventListener('click', () => { if ($('nb-stop').getAttribute('aria-disabled') !== 'true') stop(); });
  $('nb-restart').addEventListener('click', () => restartKernel(false));
  $('nb-add-code').addEventListener('click', () => {
    const c = selectedCell();
    editCell(insertCell(c ? indexOf(c) + 1 : nb.cells.length, 'code', ''));
  });
  $('nb-end-code').addEventListener('click', () => editCell(insertCell(nb.cells.length, 'code', '')));
  $('nb-end-text').addEventListener('click', () => editCell(insertCell(nb.cells.length, 'markdown', '')));

  const menu = $('nb-menu'), moreBtn = $('nb-more');
  const menuItem = act => menu.querySelector('[data-act="' + act + '"]');
  // the items shown: Settings only with git's HTTP transport (the
  // Console's #settings too), as the theme is named on its item
  const items = () => [...menu.querySelectorAll('[role="menuitem"]')].filter(b => !b.hidden);
  function updateMenu() {
    const s = menuItem('settings');
    if (s) s.hidden = !P.gitHttp();
    const t = menuItem('theme');
    if (t) t.textContent = 'Theme: ' + P.themeLabel();
  }
  function openMenu() {
    updateMenu();
    menu.hidden = false;
    moreBtn.setAttribute('aria-expanded', 'true');
    items()[0].focus();
  }
  function closeMenu(refocus) {
    if (menu.hidden) return;
    menu.hidden = true;
    moreBtn.setAttribute('aria-expanded', 'false');
    if (refocus) moreBtn.focus();
  }
  moreBtn.addEventListener('click', () => (menu.hidden ? openMenu() : closeMenu(true)));
  moreBtn.addEventListener('keydown', e => {
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') { e.preventDefault(); openMenu(); if (e.key === 'ArrowUp') items().at(-1).focus(); }
  });
  menu.addEventListener('keydown', e => {
    const list = items(), i = list.indexOf(document.activeElement);
    let j = null;
    if (e.key === 'ArrowDown') j = (i + 1) % list.length;
    else if (e.key === 'ArrowUp') j = (i - 1 + list.length) % list.length;
    else if (e.key === 'Home') j = 0;
    else if (e.key === 'End') j = list.length - 1;
    else if (e.key === 'Escape') { e.preventDefault(); closeMenu(true); return; }
    else if (e.key === 'Tab') { closeMenu(false); return; }
    if (j !== null) { e.preventDefault(); list[j].focus(); }
  });
  menu.addEventListener('click', e => {
    const b = e.target.closest('[role="menuitem"]');
    if (!b || b.getAttribute('aria-disabled') === 'true') return;   // disabled items stay focusable
    closeMenu(true);
    menuAction(b.dataset.act);
  });
  document.addEventListener('mousedown', e => {
    if (!menu.hidden && !menu.contains(e.target) && !moreBtn.contains(e.target)) closeMenu(false);
  });

  function menuAction(act) {
    switch (act) {
    case 'add-text': editCell(insertCell(indexOf(selectedCell()) + 1, 'markdown', '')); break;
    case 'restart-run': restartKernel(true); break;
    case 'clear-all':
      for (const c of nb.cells) {
        if (c.status === 'running' || c.status === 'waiting' || c.status === 'queued') continue;
        clearOutputs(c);
        c.count = null; c.status = 'idle'; c.stale = false; c.ranSource = null;
        clearHighlight(c);
        chrome(c);
      }
      announce('All outputs cleared');
      scheduleSave();
      break;
    case 'new':
      if (hasContent() && !confirm('Start a new, empty notebook? The current one is replaced; export it first to keep it.')) return;
      load({ title: 'Untitled', cells: [{ type: 'code', source: '' }] });
      editCell(nb.cells[0]);
      break;
    case 'example':
      if (hasContent() && !confirm('Replace the current notebook with the example? Export it first to keep it.')) return;
      load(S.example());
      break;
    case 'import': $('nb-file').click(); break;
    case 'export-st': exportSt(); break;
    case 'export-json': exportJson(); break;
    case 'upload': $('nb-upload').click(); break;
    case 'stop-background': stopBackground(); break;
    case 'theme': P.cycleTheme(); updateMenu(); announce('Theme: ' + P.themeLabel()); break;
    case 'settings': P.openSettings(); break;
    case 'keys': openShortcuts(); break;
    }
  }
  const hasContent = () => nb.cells.some(c => c.source.trim());
  function fileName(ext) {
    return (L.slug(nb.title || 'notebook').replace(/^section$/, 'notebook') || 'notebook') + ext;
  }
  function download(name, text, type) {
    const url = URL.createObjectURL(new Blob([text], { type: type + ';charset=utf-8' }));
    const a = h('a', { href: url, download: name, hidden: true });
    document.body.appendChild(a);
    a.click();
    a.remove();
    setTimeout(() => URL.revokeObjectURL(url), 30000);
    announce('Exported ' + name);
  }

  // the focus in a notice or the toast that goes moves to the selected cell
  function focusSelected() {
    if (selectedId && views.has(selectedId)) select(selectedId, { focus: 'cell' });
  }

  let noticeKind = null;
  function showNotice(kind, text, action, alt) {
    noticeKind = kind;
    $('nb-notice-text').textContent = text;
    const set = (b, a) => {
      b.hidden = !a;
      if (a) {
        b.textContent = a[0];
        b.onclick = () => {
          const had = $('nb-notice').contains(document.activeElement);
          hideNotice();
          a[1]();
          if (had) focusSelected();       // (the action may have replaced the cells)
        };
      }
    };
    set($('nb-notice-action'), action);
    set($('nb-notice-alt'), alt);
    $('nb-notice').hidden = false;
    announce(text);
  }
  function hideNotice(kind) {
    if (kind && kind !== noticeKind) return;
    const had = $('nb-notice').contains(document.activeElement);
    $('nb-notice').hidden = true;
    noticeKind = null;
    if (had) focusSelected();
  }
  $('nb-notice-close').addEventListener('click', () => hideNotice());

  // the toast stays while it has the focus or the pointer
  const toastEl = $('nb-toast');
  let toastTimer = 0, toastMs = 3000;
  const toastHeld = () => toastEl.matches(':hover') || toastEl.contains(document.activeElement);
  function armToast() {
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => { if (!toastHeld()) hideToast(); }, toastMs);
  }
  for (const e of ['focusin', 'mouseenter']) toastEl.addEventListener(e, () => clearTimeout(toastTimer));
  for (const e of ['focusout', 'mouseleave'])
    toastEl.addEventListener(e, () => setTimeout(() => { if (!toastEl.hidden && !toastHeld()) armToast(); }, 0));
  function toast(text, actionLabel, action, ms) {
    const b = $('nb-toast-action');
    $('nb-toast-text').textContent = text;
    b.hidden = !action;
    if (action) { b.textContent = actionLabel; b.onclick = action; }
    toastEl.hidden = false;
    toastMs = ms || 3000;
    armToast();
  }
  function hideToast() {
    clearTimeout(toastTimer);
    const had = toastEl.contains(document.activeElement);
    toastEl.hidden = true;
    if (had) focusSelected();
  }

  let liveTimer = 0;
  function announce(text) {
    const live = $('nb-live');
    live.textContent = '';
    clearTimeout(liveTimer);
    liveTimer = setTimeout(() => { live.textContent = text; }, 30);
  }

  // the kernel's messages: what it wrote outside a cell, protocol errors,
  // what the client says (kernel restarted, did not respond)
  let logLines = 0;
  function log(kind, text) {
    const pre = $('nb-log-text');
    const t = String(text).replace(/\n$/, '');
    if (!t) return;
    const line = (kind === 'stdout' || kind === 'stderr' ? '' : kind + ': ') + t + '\n';
    pre.appendChild(document.createTextNode(line));
    logLines += P.countLines(line);
    while (logLines > LOG_LINES && pre.firstChild) { logLines -= P.countLines(pre.firstChild.nodeValue); pre.firstChild.remove(); }
    $('nb-log').hidden = false;
    $('nb-log-summary').textContent = 'Kernel messages (' + logLines + ')';
  }

  // ---- shortcuts dialog

  const keysDlg = $('nb-shortcuts');
  let keysReturn = null;
  function openShortcuts() {
    keysReturn = document.activeElement;
    keysDlg.showModal();
  }
  keysDlg.addEventListener('close', () => { if (keysReturn && keysReturn.isConnected) keysReturn.focus(); });

  // ---- import, uploads

  // .json (with outputs) or .st (chunks, "%%" cells, NotebookSt.fromSt)
  $('nb-file').addEventListener('change', async e => {
    const f = e.target.files[0];
    e.target.value = '';
    if (!f) return;
    if (f.size > L.LIMITS.file) { showNotice('import', 'Could not import ' + f.name + ': the file is larger than 5 MB.'); return; }
    let res;
    try {
      const text = await f.text();
      res = /\.json$/i.test(f.name) || /^\s*\{/.test(text) ? L.fromJson(text) : S.fromSt(text);
      if (res.cells.length > L.LIMITS.cells) throw new Error('more than ' + L.LIMITS.cells + ' cells');
    } catch (err) {
      showNotice('import', 'Could not import ' + f.name + ': ' + ((err && err.message) || err));
      return;
    }
    if (!res.cells.length) { showNotice('import', 'Could not import ' + f.name + ': it has no cells.'); return; }
    if (hasContent() && !confirm('Replace the current notebook with ' + f.name + '? Export it first to keep it.')) return;
    load({ title: res.title || f.name.replace(/\.[^.]*$/, ''), created: res.created, cells: res.cells }, true);
    if (res.warnings && res.warnings.length) showNotice('import', res.warnings.join(' '));
    announce('Imported ' + f.name + ': ' + res.cells.length + ' cells');
  });

  // into the kernel's /pharo, now when it runs, and into every new one
  async function upload(files) {
    if (!files.length) return;
    if (unavailable() || !K) { unavailableNow(); return; }
    const names = [];
    for (const f of files) {
      const name = f.name.replace(/^.*[\\/]/, '') || 'upload';
      let data;
      try { data = new Uint8Array(await f.arrayBuffer()); }
      catch (e) { showNotice('upload', 'Could not read ' + name + ': ' + ((e && e.message) || e)); continue; }
      uploads.set(name, data);
      names.push(name);
      // (a kernel not running gets it when it starts: files)
      const why = kernel && kernelState !== 'off' && kernelState !== 'dead' && await kernel.writeFile(HOME + name, data);
      if (why) log('kernel', 'could not write ' + name + ': ' + why);
    }
    if (!names.length) return;
    toast('Uploaded ' + clip(names.join(', '), 60) + '. File it in with \'' + names[0] + '\' asFileReference fileIn.', null, null, 6000);
  }
  $('nb-upload').addEventListener('change', e => {
    const files = [...e.target.files];
    e.target.value = '';
    upload(files);
  });
  // Files dropped on the tab are uploaded.  page.js's handlers, on the
  // capture phase of window, see the drag first: they count it, but show
  // their overlay and open an image only while the Console's tab shows, and
  // leave dragover over this panel to these.
  const hasFiles = e => !!e.dataTransfer && [...e.dataTransfer.types].includes('Files');
  panel.addEventListener('dragover', e => {
    if (!hasFiles(e)) return;
    e.preventDefault();
    e.stopPropagation();
    e.dataTransfer.dropEffect = 'copy';
    panel.classList.add('drop');
  });
  panel.addEventListener('dragleave', e => { if (!panel.contains(e.relatedTarget)) panel.classList.remove('drop'); });
  panel.addEventListener('drop', e => {
    panel.classList.remove('drop');
    if (!hasFiles(e)) return;
    e.preventDefault();
    e.stopPropagation();
    upload([...e.dataTransfer.files]);
  });

  // the git proxy goes to a running kernel at once
  addEventListener('pharo:settings', e => {
    if (kernel) kernel.setGitProxy(e.detail ? e.detail.gitProxy : P.gitProxy());
    updateMenu();
  });
  addEventListener('pharo:theme', updateMenu);
  // the Console saved, reset, lost or replaced the image that the kernel booted
  addEventListener('pharo:image-changed', () => {
    if (!kernel || kernelState === 'off' || kernelState === 'dead' || kernelState === 'unavailable') return;
    showNotice('image', 'The Console\'s image changed: Restart the kernel to use it.', ['Restart', () => restartKernel(false)]);
  });

  // ---- tab activation

  let visited = false;
  function activated() {
    resizeAll();
    if (!visited) {
      visited = true;
      if (!unavailable() && K) startKernel();
    }
  }
  addEventListener('pharo:tab', e => {
    if (!e.detail || e.detail.tab !== 'notebook') { closeMenu(false); return; }
    activated();
  });

  // ---- persistence

  function stripOutput(o) {             // without the DOM text node
    const r = {};
    for (const k in o) if (k[0] !== '_') r[k] = o[k];
    return r;
  }
  function capOutputs(outs, max) {
    if (max === Infinity) return outs.map(stripOutput);
    let left = max;
    const r = [];
    for (const o of outs) {
      if (o.transient) continue;
      const size = o.k === 'error' ? JSON.stringify(o.error).length : String(o.text != null ? o.text : o.data != null ? o.data : '').length;
      if (size <= left) { r.push(stripOutput(o)); left -= size; continue; }
      if ((o.k === 'stream' || o.k === 'value') && left > 200 && o.text != null) {
        r.push(Object.assign(stripOutput(o), { text: o.text.slice(0, left) }));
      }
      r.push({ k: 'note', text: '… more output was not saved' });
      break;
    }
    return r;
  }
  function snapshot(withOutputs, max) {
    return {
      title: nb.title, created: nb.created, modified: nb.modified, pharo: nb.pharo || null,
      cells: nb.cells.map(c => {
        if (c.type !== 'code') return { id: c.id, type: c.type, source: c.source, count: null, status: null, outputs: [] };
        const outputs = withOutputs ? capOutputs(c.outputs, max || SAVE_OUT_MAX) : [];
        // a cell still running has partial outputs: it is saved as interrupted
        if (c.status === 'running' || c.status === 'waiting') {
          if (withOutputs) outputs.push({ k: 'note', text: 'Unfinished when the notebook was saved.' });
          return { id: c.id, type: c.type, source: c.source, count: c.runCount != null ? c.runCount : c.count,
                   status: 'interrupted', outputs };
        }
        return { id: c.id, type: c.type, source: c.source, count: c.count,
                 status: c.status === 'queued' ? c.prevStatus : c.status, outputs };
      }),
    };
  }

  // Export .json: every output, unless the file would be larger than
  // Import accepts (L.LIMITS.file, in UTF-8 bytes as File.size counts
  // them); then the outputs are capped per cell, as little as needed.
  const EXPORT_CAPS = [Infinity, 1024 * 1024, 256 * 1024, SAVE_OUT_MAX, 16 * 1024, 4096, 0];
  // Nor more outputs per cell than Import keeps (L.LIMITS.outputs): the
  // first ones, a note and the last one, as Import would.
  const utf8Bytes = s => new TextEncoder().encode(s).length;
  // after an export: a notice if Import would refuse the file (TOOBIG,
  // the notice for a file over 5 MB, or more than 5000 CELLS); true then
  function exportRefused(tooBig, cells) {
    if (tooBig) showNotice('export', tooBig);
    else if (cells > L.LIMITS.cells)
      showNotice('export', 'This notebook has more than ' + L.LIMITS.cells + ' cells: Import cannot read the file back.');
    else return false;
    return true;
  }
  // Export .st: chunks that file in as a script, when the cells declare
  // their variables (| x |) or use globals
  function exportSt() {
    const s = S.toSt(snapshot(false));
    download(fileName('.st'), s, 'text/plain');
    const big = utf8Bytes(s) > L.LIMITS.file;   // else: the cells Import finds in it
    exportRefused(big && 'This notebook is larger than 5 MB: Import cannot read the file back.',
                  big ? 0 : S.fromSt(s).cells.length);
  }
  function exportJson() {
    let s, cap, fits = false, cut = false;
    for (cap of EXPORT_CAPS) {
      const snap = cap ? snapshot(true, cap) : snapshot(false);
      cut = false;
      for (const c of snap.cells) {
        const outs = L.limitOutputs(c.outputs, L.LIMITS.outputs);
        if (outs !== c.outputs) { c.outputs = outs; cut = true; }
      }
      s = L.toJson(snap);
      if ((fits = utf8Bytes(s) <= L.LIMITS.file)) break;
    }
    download(fileName('.json'), s, 'application/json');
    if (exportRefused(!fits && 'This notebook is larger than 5 MB even without its outputs: Import cannot read the file back.',
                      nb.cells.length)) return;
    if (cap !== Infinity)
      toast('Some outputs were left out, to keep the file within the 5 MB that Import reads.', null, null, 6000);
    else if (cut)
      toast('Some outputs were left out: Import reads at most ' + L.LIMITS.outputs + ' per cell.', null, null, 6000);
  }

  let saveTimer = 0, saveFailed = false, lastSaved = null;
  function scheduleSave() {
    clearTimeout(saveTimer);
    saveTimer = setTimeout(saveNow, 600);
  }
  function write(s) {
    try { localStorage.setItem(SAVE_KEY, s); lastSaved = s; return true; } catch (e) { return false; }
  }
  function saveNow() {
    clearTimeout(saveTimer);
    saveTimer = 0;
    nb.modified = new Date().toISOString();
    if (!nb.created) nb.created = nb.modified;
    let s = L.toJson(snapshot(true));
    let ok = s.length <= SAVE_MAX && write(s);
    if (!ok) ok = write(L.toJson(snapshot(false)));
    if (!ok && !saveFailed) {
      showNotice('save', 'Autosave failed: browser storage is full or blocked. Export the notebook to keep your work.',
                 ['Export .json', () => menuAction('export-json')]);
    } else if (ok && saveFailed) hideNotice('save');
    saveFailed = !ok;
  }
  addEventListener('pagehide', () => { if (saveTimer) saveNow(); });
  document.addEventListener('visibilitychange', () => { if (document.visibilityState === 'hidden' && saveTimer) saveNow(); });
  // two saved notebooks that differ at most in when they were saved
  function sameSave(a, b) {
    if (a === b) return true;
    if (!a || !b) return false;
    try {
      const x = JSON.parse(a), y = JSON.parse(b);
      if (x.meta) x.meta.modified = null;
      if (y.meta) y.meta.modified = null;
      return JSON.stringify(x) === JSON.stringify(y);
    } catch (e) { return false; }
  }
  // another tab of this site saved its notebook
  addEventListener('storage', e => {
    if (e.key !== SAVE_KEY || !e.newValue || sameSave(e.newValue, lastSaved)) return;
    // Load theirs loads what the other tab saved then, not what storage
    // holds when it is clicked: this tab's autosave may have replaced it
    // since (and the other tab may be gone), so it is saved again.
    const theirs = e.newValue;
    showNotice('storage', 'This notebook was changed in another tab.',
               ['Load theirs', () => { if (restore(theirs, false)) write(theirs); }],
               ['Keep mine', () => saveNow()]);
  });

  // {title, created, cells: [{id?, type, source, count?, status?, outputs?}]}
  function load(doc, stale) {
    if (kernel) {                       // the cells go
      if (kernel.current && byId(kernel.current)) kernel.stop();
      for (const c of nb.cells) if (c.status === 'queued') kernel.cancel(c.id);
    }
    trash.length = 0;                   // deleted cells belong to the notebook replaced
    hideToast();
    mdIds.clear();
    mdIdsOf.clear();
    for (const v of views.values()) v.li.remove();
    views.clear();
    dirty.clear();
    nb.title = doc.title || 'Untitled';
    nb.created = doc.created || null;
    nb.cells = [];
    const seen = new Set();
    for (const d of doc.cells) {
      // as the editor has it: a lone \r would be a line break there, not in the overlay
      const c = newCell(d.type === 'markdown' ? 'markdown' : 'code', String(d.source || '').replace(/\r\n?/g, '\n'));
      if (d.id && !seen.has(d.id)) c.id = d.id;
      seen.add(c.id);
      c.editing = c.type === 'markdown' && !c.source.trim();
      if (c.type === 'code') {
        c.count = Number.isInteger(d.count) ? d.count : null;
        c.outputs = (d.outputs || []).map(o => Object.assign({}, o));
        c.ranSource = c.count != null ? c.source : null;
        c.status = d.status === 'error' || d.status === 'interrupted' ? d.status
          : c.outputs.some(o => o.k === 'error' || (o.k === 'note' && o.bad)) ? 'error' : c.count != null ? 'ok' : 'idle';
        c.stale = !!stale && (c.count != null || c.outputs.length > 0);
      }
      nb.cells.push(c);
    }
    if (!nb.cells.length) nb.cells.push(newCell('code', ''));
    const frag = document.createDocumentFragment();
    for (const c of nb.cells) frag.appendChild(makeView(c));
    cellsEl.appendChild(frag);
    for (const c of nb.cells) updateBalance(c);
    chromeAll();
    $('nb-title').value = nb.title;
    selectedId = null;
    select(nb.cells[0].id);
    resizeAll();
    scheduleSave();
  }

  // the page's own save, which Import's size limits do not apply to
  function restore(text, first) {
    let res;
    try { res = L.fromJson(text, { limits: false }); } catch (e) { return false; }
    if (!res.cells.length) return false;
    load({ title: res.title, created: res.created, cells: res.cells }, true);
    // it is what is saved: writing it again would tell the other tabs it changed
    clearTimeout(saveTimer);
    saveTimer = 0;
    lastSaved = text;
    nb.modified = res.modified;
    if (res.pharo && !nb.pharo) nb.pharo = res.pharo;
    if (res.warnings.length) showNotice('restored', 'Restored from your last visit. ' + res.warnings.join(' '));
    else if (nb.cells.some(c => c.stale))
      showNotice('restored', 'Restored from your last visit. Outputs are from an earlier session.', ['Run all', runAll]);
    else if (!first) hideNotice('storage');
    return true;
  }

  // ---- start

  $('nb-title').addEventListener('input', () => { nb.title = $('nb-title').value; scheduleSave(); });
  $('nb-title').addEventListener('keydown', e => {
    if (e.key === 'Enter' || e.key === 'Escape') { e.preventDefault(); const c = selectedCell(); if (c) select(c.id, { focus: 'cell' }); }
  });
  $('nb-info').textContent = '';
  updateMenu();

  let saved = null;
  try { saved = localStorage.getItem(SAVE_KEY); } catch (e) { saved = null; }
  if (!(saved && restore(saved, true))) {
    load(S.example());
    if (saved) showNotice('restored', 'The saved notebook could not be read, so the example was loaded.');
    clearTimeout(saveTimer);          // nothing to save until the first change
    saveTimer = 0;
  }

  if (unavailable()) lockOut();
  else if (!K) {
    fail('The notebook kernel (nb-kernel.js) did not load. Reload the page.');
  }
  // the tab may have been shown (the one last used) before this script ran
  if (!panel.hidden) activated();
})();
