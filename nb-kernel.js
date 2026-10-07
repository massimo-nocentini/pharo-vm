// nb-kernel.js - the page's client of the notebook kernel (st/web-notebook.st)
//
// UMD: a CommonJS module in node (the harnesses), the global
// PharoNotebookKernel in the page.  No DOM: it needs only a Worker-like
// object running vm-worker.js, which it starts in the mode 'notebook'.
//
//   const k = PharoNotebookKernel.create({
//     createWorker,   () => Worker-like (postMessage, terminate, onmessage,
//                     onerror) of vm-worker.js
//     getModule,      () => Promise<WebAssembly.Module> (optional)
//     init,           the fields of the worker's init (manifestUrl, build,
//                     sliceMs, persist, gitProxy), or () => them
//     files,          () => [{path, data}] written into the kernel's MEMFS
//                     after every spawn, before the first cell; data is a
//                     Uint8Array, an ArrayBuffer or a string
//     watchdogMs,     a Stop whose cell has not ended this long after kills
//                     the kernel (3000)
//     watchdogCapMs,  ... however much the cell writes meanwhile (10000)
//     stopOnError,    a cell that does not end "ok" or "terminated" cancels
//                     the queue (true)
//     schedule,       f => run f soon: acks are batched per call (default
//                     requestAnimationFrame while the page is visible,
//                     setTimeout 0 while it is hidden)
//     onCell(cellId, ev), onState(state, info), onLog(kind, text) });
//
// Then k.start() (a promise of hello), run(cellId, source), runMany([{cellId,
// source}]), cancel(cellId) (a queued cell), stop(), restart(),
// stopBackground() (a promise of how many processes of cells the kernel
// terminated), writeFile(path, data) (a promise of null once written, or of
// why not; it never rejects), setGitProxy(url) and dispose(); k.state,
// k.info, k.count (the last cell's), k.current (the cellId of the cell in
// flight) and k.queueLength.  The module also exports FrameReader,
// bgraToPng and pngOf (below), with crc32 and base64.
//
// The kernel is a second Pharo VM, booted read-only from the image that the
// Console saved in this browser (or the manifest's), which files in
// st/web-notebook.st (vmArgs('notebook')).  It is started lazily, and runs
// one cell at a time: run() queues, and the next cell is sent only after the
// kernel reported "done" for the last.  Requests are JSON lines on the VM's
// stdin ({"op":"run","rid":R,"name":"In[R]","code":...}, with rid the cell's
// count, ping and stop-background); events are JSON lines on /dev/nbevents,
// which the worker posts as output {fd: 3, bytes}.  An event with "bytes": N
// is followed by N raw bytes and an LF (FrameReader).  Every event is
// untrusted (a cell can write them too): the headers are checked before
// anything is allocated, and a frame that makes no sense is reported to
// onLog('protocol', ...), never thrown.  Events are delivered in the order
// the kernel wrote them, also when an image needs an asynchronous conversion
// (a promise chain), and so are the cell's stdout and stderr, which the
// worker posts in the VM's write order.
//
// Per run(), onCell gets, in this order,
//   queued {position} -> start {count, name} ->
//     (stream {name, text} | display {mime, data, id, size, width, height}
//      | clear | input {waiting} | late {kind, count, ...})*
//     -> done {status, count, ms, wallMs, values, truncated, error, code,
//              reason, message}
// or queued -> cancelled {reason}; run() resolves with the done or cancelled
// event and never rejects.  A display has a size when its data was too large
// to send (the page shows no more than 4 MB): the number of characters (or
// bytes), and data is empty.  An image comes as image/png in base64, with its
// width and height: the kernel sends a Form's 32-bit words (B G R A, maybe
// premultiplied), which bgraToPng encodes; without CompressionStream it is a
// text/plain display that says the image could not be converted.  input
// {waiting: true} says that the cell reads stdin, which the notebook does not
// give (only Stop ends it).  done.status is ok, error, syntax (nothing ran),
// interrupted or terminated (the cell's process terminated itself), from the
// kernel, or killed (reason: unresponsive, restart or disposed), exited
// (code) or crashed (message), when the kernel went away.  cancelled.reason is
// stopped, previous cell failed, restart, kernel exited, kernel crashed,
// kernel unavailable, removed or disposed.
//
// late {kind, count, ...} is what a process that an earlier cell forked
// sends, for that cell, the one of the same count in this kernel's lifetime:
// kind stream {name, text}, display {...}, clear, or error {error}, an
// unhandled error that terminated such a process (also one forked by the
// running cell; it does not end the cell).  Without such a cell, streams and
// errors go to onLog.
//
// onState(state, {queued, current, version, reason}): off, starting, idle,
// busy, sleeping, input (the cell waits for stdin), dead (exited, crashed or
// killed; the next run() respawns it) or unavailable.  info is hello's
// {proto, version, major, minor, image, wordSize} and ready's {source,
// savedAt, gitHttp}: source is 'saved' (the Console's image) or 'download'.
//
// onLog(kind, text): stdout and stderr outside any cell (and late streams
// and errors of no known cell), display (a display of no cell), kernel
// (restarts and exits) and protocol (anything the kernel sent that makes no
// sense).
//
// Stop is the worker's "interrupt" (the kernel's watcher stops the cell,
// st/web-notebook.st), and arms a hard deadline: the cell must end within
// watchdogMs.  The worker's tick and state messages do not extend it (the VM
// keeps slicing under a loop that the watcher cannot preempt); output does,
// by watchdogMs each time, up to watchdogCapMs after the Stop, so that the
// ensure: blocks of the cell can print.  Then the worker is terminated, the
// cell ends killed {reason: 'unresponsive'}, and a fresh kernel is started.
//
// Output is credit based (vm-worker.js): the client acks the characters of
// fd 1 and 2 and the bytes of fd 3 as they arrive, once per schedule() call.

(function (root, factory) {
  if (typeof module === 'object' && module.exports) module.exports = factory();
  else root.PharoNotebookKernel = factory();
})(typeof self !== 'undefined' ? self : this, function () {
  'use strict';

  // vm-driver.js's states
  const WAITING = 1, BUSY = 2, SLEEPING = 4;
  // The kernel caps raw images at 16 MiB; the slack is for a header's worth
  const MAX_ATTACHMENT = 16 * 1024 * 1024 + 64 * 1024;
  const MAX_SIDE = 16384;
  // The longest event line: a 4 MB display (4194304 characters), all of
  // them escaped as \u00XX, and the rest of its fields
  const MAX_LINE = 6 * 4194304 + 65536;

  const now = () => (typeof performance !== 'undefined' ? performance.now() : Date.now());
  const reasonOf = e => String((e && (e.reason || e.message)) || e || 'unknown error');
  const noErrorResult = () => ({
    text: 'Error: the cell ended without reporting a result',
    kind: [], location: null, form: null, line: null, column: null, chain: [],
  });
  const isCount = n => Number.isInteger(n) && n >= 0;

  // ---- the frames of /dev/nbevents

  // new FrameReader(onFrame, onError): push(bytes) the Uint8Arrays of fd 3
  // as they come, split anywhere; onFrame(ev, attachment) gets every event
  // object, with its attachment (a Uint8Array of its own) or null, and
  // onError(text) every protocol error.  A header with "bytes" that fails the
  // checks (at most MAX_ATTACHMENT; an image/x-pharo-bgra with integer width
  // and height in 1..MAX_SIDE and bytes 4*width*height) is an error, and its
  // attachment is skipped by its declared length without being kept.  A
  // missing LF after an attachment is an error too, and the reader resyncs
  // at the next LF.
  class FrameReader {
    constructor(onFrame, onError) {
      this.onFrame = onFrame;
      this.onError = onError;
      this.decoder = new TextDecoder();
      this.parts = [];                  // the line so far
      this.partLen = 0;
      this.dropLine = false;            // skip to the next LF
      this.att = null;                  // the attachment being read
      this.attAt = 0;
      this.skip = 0;                    // bytes of a rejected attachment left
      this.frame = null;                // the event of the attachment
      this.needLF = false;              // an attachment ended
    }
    reset() {
      this.parts = [];
      this.partLen = 0;
      this.dropLine = this.needLF = false;
      this.att = this.frame = null;
      this.attAt = this.skip = 0;
    }
    error(text) { try { this.onError(text); } catch (e) { console.error(e); } }
    deliver(ev, att) { try { this.onFrame(ev, att); } catch (e) { console.error(e); } }
    push(chunk) {
      const n = chunk.length;
      let i = 0;
      while (i < n) {
        if (this.skip) {
          const k = Math.min(this.skip, n - i);
          this.skip -= k;
          i += k;
          if (!this.skip) this.needLF = true;
          continue;
        }
        if (this.att && !this.needLF) {
          const k = Math.min(this.att.length - this.attAt, n - i);
          this.att.set(chunk.subarray(i, i + k), this.attAt);
          this.attAt += k;
          i += k;
          if (this.attAt === this.att.length) this.needLF = true;
          continue;
        }
        if (this.needLF) {
          const ev = this.frame, att = this.att;
          this.needLF = false;
          this.frame = this.att = null;
          this.attAt = 0;
          if (chunk[i] === 10) {
            i++;
            if (ev) this.deliver(ev, att);
          } else {
            this.error('no LF after the ' + (ev ? ev.bytes : 'skipped') + ' bytes of ' +
                       (ev ? 'a ' + clip(String(ev.ev), 40) + ' event' : 'a rejected attachment'));
            this.dropLine = true;       // resync at the next LF
          }
          continue;
        }
        const lf = chunk.indexOf(10, i), end = lf < 0 ? n : lf;
        if (!this.dropLine) {
          if (this.partLen + end - i > MAX_LINE) {
            this.error('an event line longer than ' + MAX_LINE + ' bytes');
            this.parts = [];
            this.partLen = 0;
            this.dropLine = true;
          } else if (end > i) {
            // (a copy, unless the line ends here: the chunk may be reused)
            this.parts.push(lf < 0 ? chunk.slice(i, end) : chunk.subarray(i, end));
            this.partLen += end - i;
          }
        }
        if (lf < 0) break;
        i = lf + 1;
        if (this.dropLine) { this.dropLine = false; continue; }
        const line = this.takeLine();
        if (line) this.line(line);
      }
    }
    takeLine() {
      const parts = this.parts, len = this.partLen;
      this.parts = [];
      this.partLen = 0;
      if (parts.length === 1) return this.decoder.decode(parts[0]);
      const all = new Uint8Array(len);
      let at = 0;
      for (const p of parts) { all.set(p, at); at += p.length; }
      return this.decoder.decode(all);
    }
    line(text) {
      let ev;
      try { ev = JSON.parse(text); } catch (e) { this.error('not JSON: ' + clip(text, 200)); return; }
      if (!ev || typeof ev !== 'object' || Array.isArray(ev) || typeof ev.ev !== 'string') {
        this.error('not an event: ' + clip(text, 200));
        return;
      }
      if (ev.bytes === undefined) { this.deliver(ev, null); return; }
      const n = ev.bytes;
      if (!Number.isSafeInteger(n) || n < 0) {
        this.error('an attachment of ' + clip(JSON.stringify(n), 40) + ' bytes');
        return;                         // nothing to skip by
      }
      const why = attachmentProblem(ev);
      if (why) {
        this.error(why + ': ' + clip(text, 200));
        this.skip = n;
        if (!n) this.needLF = true;
        return;
      }
      this.frame = ev;
      this.att = new Uint8Array(n);
      this.attAt = 0;
      if (!n) this.needLF = true;
    }
  }

  // What is wrong with the header of an event with an attachment, or null
  function attachmentProblem(ev) {
    if (ev.bytes > MAX_ATTACHMENT) return 'an attachment of ' + ev.bytes + ' bytes, over ' + MAX_ATTACHMENT;
    if (ev.mime === 'image/x-pharo-bgra') {
      const w = ev.width, h = ev.height;
      if (!Number.isInteger(w) || !Number.isInteger(h) || w < 1 || h < 1 || w > MAX_SIDE || h > MAX_SIDE)
        return 'an image of ' + clip(String(w), 20) + 'x' + clip(String(h), 20) + ' pixels';
      if (ev.bytes !== 4 * w * h) return 'an image of ' + w + 'x' + h + ' pixels in ' + ev.bytes + ' bytes';
    }
    return null;
  }

  function clip(s, n) { return s.length > n ? s.slice(0, n) + '...' : s; }

  // ---- PNG

  let crcTable = null;
  function crc32(bytes, crc = 0) {
    if (!crcTable) {
      crcTable = new Uint32Array(256);
      for (let n = 0; n < 256; n++) {
        let c = n;
        for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
        crcTable[n] = c >>> 0;
      }
    }
    let c = ~crc >>> 0;
    for (let i = 0; i < bytes.length; i++) c = crcTable[(c ^ bytes[i]) & 255] ^ (c >>> 8);
    return ~c >>> 0;
  }

  function base64(bytes) {
    let s = '';
    for (let i = 0; i < bytes.length; i += 0x8000)
      s += String.fromCharCode.apply(null, bytes.subarray(i, i + 0x8000));
    return btoa(s);
  }

  async function deflate(data) {
    const stream = new Blob([data]).stream().pipeThrough(new CompressionStream('deflate'));
    const reader = stream.getReader(), chunks = [];
    let len = 0;
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      chunks.push(value);
      len += value.length;
    }
    const all = new Uint8Array(len);
    let at = 0;
    for (const c of chunks) { all.set(c, at); at += c.length; }
    return all;
  }

  // The PNG (RGBA, 8 bits) of w x h pixels given as the 32-bit words of a
  // Form in memory order, B G R A, as a Uint8Array; premultiplied pixels
  // (cairo's) are divided by their alpha first
  async function pngOf(w, h, bgra, premultiplied) {
    if (!(w >= 1 && h >= 1 && bgra.length === 4 * w * h)) throw new Error('an image of ' + w + 'x' + h + ' in ' + bgra.length + ' bytes');
    const row = 4 * w + 1, raw = new Uint8Array(row * h);
    for (let y = 0, i = 0; y < h; y++) {
      let o = y * row + 1;              // (filter 0, none)
      for (let x = 0; x < w; x++, i += 4, o += 4) {
        let r = bgra[i + 2], g = bgra[i + 1], b = bgra[i];
        const a = bgra[i + 3];
        if (premultiplied && a && a < 255) {
          r = Math.min(255, Math.round(r * 255 / a));
          g = Math.min(255, Math.round(g * 255 / a));
          b = Math.min(255, Math.round(b * 255 / a));
        }
        raw[o] = r; raw[o + 1] = g; raw[o + 2] = b; raw[o + 3] = a;
      }
    }
    const idat = await deflate(raw);
    const ihdr = new Uint8Array(13), hv = new DataView(ihdr.buffer);
    hv.setUint32(0, w);
    hv.setUint32(4, h);
    ihdr[8] = 8;                        // bits per sample
    ihdr[9] = 6;                        // RGBA
    const chunks = [['IHDR', ihdr], ['IDAT', idat], ['IEND', new Uint8Array(0)]];
    const png = new Uint8Array(8 + chunks.reduce((n, [, d]) => n + 12 + d.length, 0));
    png.set([137, 80, 78, 71, 13, 10, 26, 10]);
    const pv = new DataView(png.buffer);
    let at = 8;
    for (const [type, data] of chunks) {
      pv.setUint32(at, data.length);
      for (let k = 0; k < 4; k++) png[at + 4 + k] = type.charCodeAt(k);
      png.set(data, at + 8);
      pv.setUint32(at + 8 + data.length, crc32(png.subarray(at + 4, at + 8 + data.length)));
      at += 12 + data.length;
    }
    return png;
  }

  // The same PNG in base64, what an image/png display's data is
  async function bgraToPng(w, h, bgra, premultiplied) { return base64(await pngOf(w, h, bgra, premultiplied)); }

  // ---- the client

  // Run f soon: at the next animation frame while the page is visible (also
  // when it goes hidden before that frame), else at once
  function defaultSchedule(f) {
    const hidden = typeof document !== 'undefined' && document.hidden;
    if (hidden || typeof requestAnimationFrame !== 'function') { setTimeout(f, 0); return; }
    let done = false;
    const once = () => { if (!done) { done = true; f(); } };
    requestAnimationFrame(once);
    setTimeout(once, 250);
  }

  function create(opts) {
    const o = Object.assign({ watchdogMs: 3000, watchdogCapMs: 10000, stopOnError: true }, opts);
    const schedule = o.schedule || defaultSchedule;
    const call = (f, ...a) => { try { if (f) f(...a); } catch (e) { console.error(e); } };
    const onCell = (id, ev) => call(o.onCell, id, ev);
    const log = (kind, text) => call(o.onLog, kind, text);
    const get = (f, dflt) => {
      if (f === undefined || f === null) return dflt;
      const v = typeof f === 'function' ? f() : f;
      return v === undefined ? dflt : v;
    };

    let w = null, gen = 0, ready = false, hello = false, info = null, readyInfo = null;
    let state = 'off', reason = null, lastState = '';
    let queue = [], current = null, execCount = 0;
    // the cells of this kernel's lifetime by their rid, for "late" events
    let cellOf = new Map();
    // start()'s promise: one per startup, kept across a respawn before hello
    let startP = null, startRes = null, startRej = null;
    function settleStart(err) {
      const f = err ? startRej : startRes;
      startP = startRes = startRej = null;
      if (f) f(err);
    }
    let ackChars = 0, ackPending = false;
    // the frames of fd 3, and what the reader found in the last output
    let reader = null;
    const frames = [];
    // the worker's messages in order, each run once the one before is done
    let tasks = [], running = false;
    // the Stop watchdog: when the Stop was, and the deadline
    let watchdog = null, stopAt = 0, deadline = 0;
    // stopBackground()'s and writeFile()'s answers to come
    let stopWaiters = [], fsWaiters = new Map(), fsId = 0;
    let gitProxy;                       // setGitProxy()'s, else init's
    let disposed = false;

    function emitState() {
      const key = state + '|' + queue.length + '|' + (current ? current.cellId : '') + '|' + reason;
      if (key === lastState) return;
      lastState = key;
      call(o.onState, state, { queued: queue.length, current: current ? current.cellId : null,
                               version: info ? info.version : null, reason });
    }
    function setState(s, why) { state = s; reason = why || null; emitState(); }

    function post(m) { if (w) w.postMessage(m); }
    function request(req) { post({ type: 'input', text: JSON.stringify(req) + '\n' }); }

    function kill() {
      gen++;                            // later messages of this worker are ignored
      if (w) { try { w.terminate(); } catch (e) { /* gone */ } }
      w = null; ready = false; hello = false; ackChars = 0; ackPending = false; reader = null;
      tasks = [];
      frames.length = 0;
      cellOf = new Map();
      disarm();
      // nothing is left in the background, nor any file to write
      for (const f of stopWaiters) f(0);
      stopWaiters = [];
      for (const f of fsWaiters.values()) f('the kernel was stopped');
      fsWaiters = new Map();
    }

    // ---- terminal events

    function cancelQueue(why) {
      const q = queue;
      queue = [];
      for (const e of q) {
        const ev = { type: 'cancelled', reason: why };
        onCell(e.cellId, ev);
        e.resolve(ev);
      }
      emitState();
    }

    // the current cell ends, with the fields of the kernel's done or
    // synthesized ones
    function finish(fields, fromKernel) {
      const cur = current;
      current = null;
      disarm();
      showInput(cur, false);
      const ev = { type: 'done', status: String(fields.status), count: cur.count, ms: null,
                   wallMs: Math.round(now() - cur.t0), values: null, truncated: false, error: null };
      if (isCount(fields.ms)) ev.ms = fields.ms;
      if (Array.isArray(fields.values)) ev.values = fields.values.map(String);
      if (fields.truncated === true) ev.truncated = true;
      if (fields.error && typeof fields.error === 'object') ev.error = fields.error;
      for (const k of ['code', 'reason', 'message']) if (fields[k] !== undefined) ev[k] = fields[k];
      onCell(cur.cellId, ev);
      cur.resolve(ev);
      if (fromKernel && ev.status !== 'ok' && ev.status !== 'terminated' && o.stopOnError)
        cancelQueue('previous cell failed');
      return ev;
    }

    // ---- the worker

    function spawn() {
      kill();
      const g = gen;
      execCount = 0;
      info = readyInfo = null;
      if (!startP) {
        startP = new Promise((res, rej) => { startRes = res; startRej = rej; });
        startP.catch(() => {});         // run() does not need it
      }
      const p = startP;
      setState('starting');
      Promise.resolve()
        .then(() => (o.getModule ? o.getModule() : undefined))
        .then(wasmModule => {
          if (g !== gen || disposed) return;
          const worker = o.createWorker();
          w = worker;
          reader = new FrameReader((ev, att) => frames.push([ev, att]), text => frames.push([null, text]));
          worker.onmessage = e => { if (g === gen) receive(e.data, g); };
          worker.onerror = e => {
            if (g !== gen) return;
            if (e && e.preventDefault) e.preventDefault();
            const why = reasonOf(e);
            order(g, () => { if (ready) gone('crashed', why); else unavailable(why); });
          };
          worker.onmessageerror = () => { if (g === gen) log('protocol', 'a message of the worker could not be decoded'); };
          const init = Object.assign({}, get(o.init, {}));
          if (gitProxy !== undefined) init.gitProxy = gitProxy;
          worker.postMessage(Object.assign(init, { type: 'init', mode: 'notebook', wasmModule }));
          // (the worker keeps them until it is ready, and the first run
          // comes after hello)
          for (const f of get(o.files, []) || []) writeTo(f.path, f.data, true);
        })
        .catch(e => { if (g === gen) unavailable(reasonOf(e)); });
      return p;
    }

    function unavailable(why) {
      kill();
      if (current) finish({ status: 'crashed', message: why });
      setState('unavailable', why);
      cancelQueue('kernel unavailable');
      settleStart({ reason: why });
    }

    // exit or crash (or a worker error after ready)
    function gone(how, detail) {
      const wasHello = hello;
      kill();
      const why = how === 'exited' ? 'kernel exited (code ' + detail + ')' : 'kernel crashed: ' + detail;
      if (current) finish(how === 'exited' ? { status: 'exited', code: detail }
                                           : { status: 'crashed', message: String(detail) });
      cancelQueue(how === 'exited' ? 'kernel exited' : 'kernel crashed');
      setState('dead', why);
      log('kernel', why);
      if (!wasHello) settleStart({ reason: why });
    }

    // A message of the worker: its output is acked and extends the watchdog
    // at once, and everything is then handled in order
    function receive(m, g) {
      if (!m || typeof m !== 'object') return;
      if (m.type === 'output') {
        const n = m.fd === 3 ? (m.bytes ? m.bytes.length : 0) : String(m.text || '').length;
        ack(n, g);
        if (watchdog) extend();
      }
      order(g, () => message(m));
    }

    function ack(n, g) {
      ackChars += n;
      if (ackPending) return;
      ackPending = true;
      schedule(() => {
        if (g !== gen) return;          // (kill() forgot them)
        ackPending = false;
        if (ackChars) post({ type: 'ack', chars: ackChars });
        ackChars = 0;
      });
    }

    // Run f after the tasks before it, of the worker of generation g; a task
    // that answers a promise holds the next ones until it settles
    function order(g, f) {
      tasks.push(() => (g === gen ? f() : undefined));
      if (!running) drain();
    }
    function drain() {
      running = true;
      while (tasks.length) {
        const f = tasks.shift();
        let r;
        try { r = f(); } catch (e) { console.error(e); }
        if (r && typeof r.then === 'function') {
          r.then(drain, e => { console.error(e); drain(); });
          return;
        }
      }
      running = false;
    }

    function message(m) {
      switch (m.type) {
      case 'ready':
        if (m.mode !== 'notebook') {
          unavailable('vm-worker.js has no notebook mode (build ' + (m.build || 'unknown') + ')');
          break;
        }
        ready = true;
        readyInfo = { source: m.source || null, savedAt: m.savedAt || null, gitHttp: !!m.gitHttp };
        break;
      case 'output':
        if (m.fd === 3) {
          if (!m.bytes || !reader) break;
          reader.push(m.bytes instanceof Uint8Array ? m.bytes : new Uint8Array(m.bytes));
          return events(frames.splice(0), 0, gen);
        }
        if (current && current.started)
          onCell(current.cellId, { type: 'stream', name: m.fd === 2 ? 'stderr' : 'stdout', text: String(m.text) });
        else log(m.fd === 2 ? 'stderr' : 'stdout', String(m.text));
        break;
      case 'state': workerState(m.state); break;
      case 'interrupted':
        // nothing to stop the cell with: the deadline is now
        if (!m.registered && watchdog && current) expire();
        break;
      case 'fs-result': case 'error': {
        const f = fsWaiters.get(m.id);
        if (!f) break;
        fsWaiters.delete(m.id);
        f(m.type === 'error' ? String(m.message) : null);
        break;
      }
      case 'exit': gone('exited', m.code); break;
      case 'crash': if (ready) gone('crashed', m.message); else unavailable(String(m.message)); break;
      }
    }

    // A state message reports the end of a slice.  Those that come before
    // the current cell's "start" event are of earlier slices: the kernel has
    // not read the cell's request yet (the worker posts the output of a
    // slice before its state, so "start" comes first).  The idle kernel
    // waits for its stdin (WAITING); a cell that does is "input".
    function workerState(st) {
      if (!hello || !current || !current.started) return;
      const waiting = st === WAITING;
      showInput(current, waiting);
      setState(waiting ? 'input' : st === SLEEPING ? 'sleeping' : 'busy');
    }

    // CUR.shown is what onCell was last told
    function showInput(cur, on) {
      if (cur.shown === on) return;
      cur.shown = on;
      onCell(cur.cellId, { type: 'input', waiting: on });
    }

    // The events of an output of fd 3, with their attachments (or [null,
    // the text of a protocol error]), from the i-th: one whose display is
    // converted holds the rest, and the worker's messages after them, until
    // it is delivered
    function events(list, i, g) {
      for (; i < list.length && g === gen; i++) {
        const [ev, att] = list[i];
        let r;
        if (!ev) { log('protocol', att); continue; }
        try { r = handle(ev, att); } catch (e) { console.error(e); }
        if (r && typeof r.then === 'function') {
          const next = i + 1;
          return r.catch(e => console.error(e)).then(() => events(list, next, g));
        }
      }
      return undefined;
    }

    function display(ev) {
      const d = { type: 'display', mime: String(ev.mime), data: ev.data == null ? '' : String(ev.data),
                  id: ev.id == null ? null : String(ev.id) };
      if (isCount(ev.size)) d.size = ev.size;
      return d;
    }

    // The display of an event, whose attachment is converted: a promise
    async function displayOf(ev, att) {
      if (!att) return display(ev);
      const id = ev.id == null ? null : String(ev.id);
      if (ev.mime !== 'image/x-pharo-bgra') return { type: 'display', mime: String(ev.mime), data: base64(att), id };
      try {
        const data = await bgraToPng(ev.width, ev.height, att, ev.premultiplied === true);
        return { type: 'display', mime: 'image/png', data, id, width: ev.width, height: ev.height };
      } catch (e) {
        return { type: 'display', mime: 'text/plain', data: 'image could not be converted (' + reasonOf(e) + ')', id };
      }
    }

    // Where an event of rid goes: CURRENT (the running cell), the id of a
    // cell of the past (late), or null
    const CURRENT = {};
    function routeOf(rid) {
      if (current && rid === current.rid) return current.started ? CURRENT : null;
      return Number.isInteger(rid) && cellOf.has(rid) ? cellOf.get(rid) : null;
    }

    function late(cellId, kind, rid, fields) {
      onCell(cellId, Object.assign({}, fields, { type: 'late', kind, count: rid }));
    }

    function handle(ev, att) {
      switch (ev.ev) {
      case 'hello':
        if (hello && current) {         // the kernel started over under a cell
          log('protocol', 'hello during a cell');
          finish({ status: 'error', error: noErrorResult() }, true);
        }
        hello = true;
        info = Object.assign({ proto: ev.proto, version: ev.version == null ? null : String(ev.version),
                               major: ev.major, minor: ev.minor, image: ev.image == null ? null : String(ev.image),
                               wordSize: ev.wordSize }, readyInfo);
        setState('idle');
        settleStart();
        pump();
        break;
      case 'start':
        if (!current || ev.rid !== current.rid)
          log('protocol', 'start for rid ' + ev.rid + ', current ' + (current ? current.rid : 'none'));
        else current.started = true;
        break;
      case 'display':
      case 'clear': {
        const to = routeOf(ev.rid);
        if (ev.ev === 'clear') {
          if (to === CURRENT) onCell(current.cellId, { type: 'clear' });
          else if (to !== null) late(to, 'clear', ev.rid, {});
          else log('protocol', 'clear for rid ' + ev.rid + ' outside a cell');
          break;
        }
        const g = gen;
        return displayOf(ev, att).then(d => {
          if (g !== gen) return;
          const at = routeOf(ev.rid);   // (the conversion took a while)
          if (at === CURRENT) onCell(current.cellId, d);
          else if (at !== null) late(at, 'display', ev.rid, d);
          else log('display', d.mime + ' output of no cell not shown (' +
                   (d.size != null ? d.size : d.data.length) + ' characters)');
        });
      }
      case 'stream': {
        const name = ev.name === 'stderr' ? 'stderr' : 'stdout', text = String(ev.text == null ? '' : ev.text);
        const to = routeOf(ev.rid);
        if (to === CURRENT) onCell(current.cellId, { type: 'stream', name, text });
        else if (to !== null) late(to, 'stream', ev.rid, { name, text });
        else log(name, text);
        break;
      }
      case 'error': {
        const error = ev.error && typeof ev.error === 'object' ? ev.error : noErrorResult();
        const to = current && ev.rid === current.rid ? current.cellId : routeOf(ev.rid);
        if (to !== null) late(to, 'error', ev.rid, { error });
        else log('stderr', String(error.text || 'Error') + '\n');
        break;
      }
      case 'done':
        if (!current) { log('protocol', 'done for rid ' + ev.rid + ' without a current cell'); break; }
        // A done of another rid (one a cell forged, or the real one after
        // it) must not end the cell in flight with someone else's result
        if (ev.rid !== current.rid) { log('protocol', 'done for rid ' + ev.rid + ', current ' + current.rid); break; }
        finish(ev, true);
        setState('idle');
        pump();
        break;
      case 'stopped': {
        const f = stopWaiters.shift();
        if (f) f(isCount(ev.count) ? ev.count : 0);
        break;
      }
      case 'pong': break;
      case 'bad-request':
        log('protocol', 'bad request: ' + String(ev.text));
        if (current && !current.started) {      // the cell never starts
          finish({ status: 'error', error: noErrorResult() }, true);
          setState('idle');
          pump();
        }
        break;
      default: log('protocol', clip(JSON.stringify(ev), 200));
      }
    }

    function pump() {
      if (!hello || current || !queue.length || disposed) return;
      const q = queue.shift();
      const count = ++execCount, rid = count, name = 'In[' + count + ']';
      current = { cellId: q.cellId, rid, count, resolve: q.resolve, t0: now(), shown: false, started: false };
      cellOf.set(rid, q.cellId);
      const src = typeof q.source.toWellFormed === 'function' ? q.source.toWellFormed() : q.source;
      request({ op: 'run', rid, name, code: src });
      onCell(q.cellId, { type: 'start', count, name });
      setState('busy');
    }

    function spawnIfNeeded() {
      if (state === 'dead') log('kernel', 'kernel restarted');
      if (state === 'off' || state === 'unavailable' || state === 'dead') return spawn();
      return startP || Promise.resolve();
    }

    // ---- files

    function writeTo(path, data, quiet) {
      return new Promise(resolve => {
        if (!w) { resolve('the kernel is not running'); return; }
        const id = ++fsId;
        fsWaiters.set(id, why => {
          if (why && quiet) log('kernel', 'could not write ' + path + ': ' + why);
          resolve(why);
        });
        post({ type: 'fs', id, op: 'writeFile', path: String(path), data });
      });
    }

    // ---- the Stop watchdog

    // Armed by stop(): the current cell must end by the deadline.  Only
    // output extends it, up to watchdogCapMs after the Stop.
    function disarm() { if (watchdog) { clearTimeout(watchdog); watchdog = null; } }
    function arm() {
      if (watchdog) return;             // a second Stop does not extend it
      stopAt = now();
      deadline = stopAt + o.watchdogMs;
      wait();
    }
    function extend() { deadline = Math.min(Math.max(deadline, now() + o.watchdogMs), stopAt + o.watchdogCapMs); }
    function wait() {
      watchdog = setTimeout(() => {
        watchdog = null;
        if (!current) return;
        if (now() < deadline) wait(); else expire();
      }, Math.max(1, Math.ceil(deadline - now())));
    }
    function expire() {
      disarm();
      kill();
      finish({ status: 'killed', reason: 'unresponsive' });
      cancelQueue('stopped');
      log('kernel', 'kernel did not respond to Stop and was restarted');
      spawn();
    }

    // ---- the API

    const api = {
      start() {
        if (disposed) return Promise.reject({ reason: 'disposed' });
        if (hello) return Promise.resolve();
        if (state === 'starting' && startP) return startP;
        return spawnIfNeeded();
      },
      run(cellId, source) {
        return new Promise(resolve => {
          if (disposed) {
            const ev = { type: 'cancelled', reason: 'disposed' };
            onCell(cellId, ev);
            resolve(ev);
            return;
          }
          queue.push({ cellId, source: String(source), resolve });
          onCell(cellId, { type: 'queued', position: queue.length });
          spawnIfNeeded();
          emitState();
          pump();
        });
      },
      runMany(cells) { return Promise.all(cells.map(c => api.run(c.cellId, c.source))); },
      cancel(cellId) {
        const keep = [];
        for (const e of queue) {
          if (e.cellId === cellId) {
            const ev = { type: 'cancelled', reason: 'removed' };
            onCell(e.cellId, ev);
            e.resolve(ev);
          } else keep.push(e);
        }
        queue = keep;
        emitState();
      },
      stop() {
        if (current && w) {
          post({ type: 'interrupt' });
          arm();
        }
        cancelQueue('stopped');
      },
      restart() {
        if (disposed) return Promise.reject({ reason: 'disposed' });
        const was = state;
        kill();
        if (current) finish({ status: 'killed', reason: 'restart' });
        cancelQueue('restart');
        if (was !== 'off' && was !== 'unavailable') log('kernel', 'kernel restarted');
        return spawn();
      },
      // Terminate the processes that cells forked: answers how many (0 when
      // there is no kernel).  Served after the running cell.
      stopBackground() {
        if (!hello || !w) return Promise.resolve(0);
        return new Promise(resolve => {
          stopWaiters.push(resolve);
          request({ op: 'stop-background' });
        });
      },
      // Write a file of the kernel's MEMFS (the worker keeps it until it is
      // ready): answers null once written, or why it was not; never rejects
      writeFile(path, data) { return writeTo(path, data, false); },
      setGitProxy(p) {
        gitProxy = typeof p === 'string' ? p : '';
        post({ type: 'gitProxy', gitProxy });
      },
      dispose() {
        if (disposed) return;
        disposed = true;
        kill();
        if (current) finish({ status: 'killed', reason: 'disposed' });
        cancelQueue('disposed');
        settleStart({ reason: 'disposed' });
        setState('off');
      },
      get state() { return state; },
      get current() { return current ? current.cellId : null; },
      get queueLength() { return queue.length; },
      get count() { return execCount; },
      get execCount() { return execCount; },
      get info() { return info; },
    };
    return api;
  }

  return { create, FrameReader, bgraToPng, pngOf, crc32, base64, MAX_ATTACHMENT, MAX_SIDE };
});
