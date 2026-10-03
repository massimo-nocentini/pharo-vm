// vm-worker.js - run the Pharo VM for WebAssembly in a Web Worker
//
// The page owns one worker per VM: page.js (the Console) and, in M2,
// world.js (the world).  Messages:
//
//   page -> worker   init {wasmModule, manifestUrl, build, mode, sliceMs,
//                          persist, upload: {image, changes}, display},
//                    input {text}, eof, interrupt, save, ack {chars},
//                    fs {id, op, path, data}, download {id},
//                    resetStorage {id}, flush, display {...}
//   worker -> page   progress {phase, loaded, total},
//                    ready {image, source, persisted, savedAt, world,
//                           prepared, storageError},
//                    output {fd, text}, state {state, waiting}, tick,
//                    interrupted {registered}, edited {edited},
//                    storing, saved {bytes, prepared, error, upload},
//                    superseded, reset {id, error}, exit {code},
//                    crash {message, stack, stacks}, fs-result {id, ...},
//                    file {id, name, data, count}, error {id, message},
//                    display {...}
//
// Messages that arrive before "ready" are queued and replayed after it;
// "ready" always precedes the first "state".  When the VM cannot start, or
// the boot fails, the worker says "crash" instead, and still answers what
// needs no VM (resetStorage, whose storage is opened first of all, so that it
// deletes the slot even when the manifest could not be fetched).  After
// "exit" and "crash" it stays, so that the files of the VM can still be read
// and downloaded, until the page terminates it.
//
// Boot.  init.manifestUrl is web/manifest.json, written by stage.mjs.  The
// image and its .changes come from init.upload, else from the slot of
// vm-storage.js when init.persist and there is one, else from the manifest,
// like the .sources and the st files, which always do.  Every file of the
// manifest is fetched and inflated as it comes (DecompressionStream) into a
// buffer of its size, unless its first bytes are not gzip's 1f 8b: a server
// that sent it with Content-Encoding: gzip had the browser inflate it.
// Everything goes into /pharo, the working directory and the VM's directory
// (thisProgram is /pharo/pharo), which must be writable.  The VM then boots
// with PharoVMDriver.vmArgs(init.mode): the REPL of st/web-repl.st, or the
// world.  progress says how far the loading got: "fetch" (the bytes of the
// files, inflated), "restore" (the slot), then "boot".
//
// Output.  What the VM writes to fd 1 and 2 is decoded as UTF-8 (one
// streaming decoder per fd), coalesced up to 64 KB, and posted at the end
// of every slice.  It is credit based: the page acks the characters it
// rendered, and the VM is paused (between slices) while more than 1 MiB are
// unacknowledged, until fewer than 512 KiB are.  "state" comes when the
// state changes or after the page sent something; while the VM is BUSY and
// nothing else was posted for a second, "tick" says that it is alive.
//
// Persistence.  When the VM has written an image (HOST_IMAGE_SAVED, which
// comes after the slice that saved it), the worker reads it and its .changes
// from /pharo, says "storing", stores them as the new slot and says "saved".
// The slot says whether its image can open the world (prepared): in the
// Console the REPL tells, in the file WORLD_FILE (st/web-repl.st), else the
// name of the class of OSWindow-Web must be in the bytes of the image.  An
// upload becomes the slot only once it has booted: when the REPL first waits for
// input (the world: first sleeps), with "saved {upload: true}"; one that
// crashes or quits before leaves the slot as it was.  ready.persisted says
// whether there is a slot: the one booted, the one that an upload would
// replace, or one that could not be read.  The .changes of the slot that
// the worker booted from or saved is stored again when it changed: SETTLE_MS
// after the VM went idle (the REPL waits for input, the world sleeps), at
// the latest SYNC_MS after it was last stored, on "flush" (the page goes
// away, which a worker may not outlive: little is left for it), and when the
// VM exits; "exit" is posted once the storage is done.  Only while that
// slot is still the stored one, though: once another page (a second tab,
// the world) saved over it or reset it, the .changes of this VM would not
// match the image stored, so the worker stops and says "superseded", until
// its VM saves again.  A failure of the storage is reported in "saved",
// never fatal.  "save" types Smalltalk snapshot: true andQuit: false into
// the REPL; the image itself may save at any time too.  In the Console,
// "edited" says whether the .changes of the VM differs from the one it had
// when it first waited for input, or when its image was last stored: code
// was changed that the slot does not have.
//
// M2.  With init.display the worker loads display-worker.js, whose
// PharoDisplay.create(init.display, post) is the webDisplay of the VM.  It
// hands the display the driver as soon as the VM is created, with the
// message {kind: 'attach'} (which asks for nothing else), and then the
// "display" messages of the page, with PharoDisplay.onMessage.
//
// The worker URL's ?v= query (the build id) is passed on to every script.

'use strict';

const V = new URLSearchParams(self.location.search).get('v') || '';
const q = V ? '?v=' + encodeURIComponent(V) : '';
importScripts('pharo-web.js' + q, 'vm-driver.js' + q, 'vm-storage.js' + q);

// A zero-delay macrotask: setTimeout(f, 0) is clamped to 4 ms once nested.
const ch = new MessageChannel();
let job = null;
ch.port1.onmessage = () => { const j = job; job = null; if (j) j(); };
const schedule = f => { job = f; ch.port2.postMessage(0); };
const later = (f, ms) => setTimeout(f, ms);

const { BUSY, WAITING, SLEEPING, HOST_IMAGE_SAVED } = PharoVMDriver;
const HIGH = 1 << 20;                   // pause above this many unacked characters
const COALESCE = 65536;
const TICK_MS = 1000, SYNC_MS = 5000, SETTLE_MS = 500;
const DIR = '/pharo';
const SAVE = 'Smalltalk snapshot: true andQuit: false';
// The class of the OSWindow-Web package that a world image has
const WORLD_CLASS = 'OSWebDriver';
// Where the REPL says whether its image has it (PHARO_WEB_WORLD_FILE)
const WORLD_FILE = DIR + '/.pharo-web-world';

let drv = null, settled = false, queue = [];
let mode = null, manifest = null, display = null;
let store = null, storageError = null;
// the meta of the slot that this VM booted from or saved, while it is the
// stored one
let owned = null, syncTimer = 0, syncedKey = '', syncedAt = 0, settleTimer = 0;
// the .changes when the VM first waited for input or its image was stored
// (null before), and whether it differs now ("edited")
let editedBase = null, edited = false;
// {image, changes, prepared}: the upload that this VM booted, not stored yet
let upload = null;
let imagePath = DIR + '/Pharo.image', changesPath = DIR + '/Pharo.changes';
let storing = Promise.resolve();        // the storage work, in order
let out = [], outLen = 0, unacked = 0;
let lastState = null, stateDirty = true, lastPostAt = 0;
const decoders = { 1: new TextDecoder(), 2: new TextDecoder() };

function post(m, transfer) {
  lastPostAt = performance.now();
  postMessage(m, transfer || []);
}

const changesOf = path => path.replace(/\.image$/, '') + '.changes';
// the bytes of a Uint8Array or an ArrayBuffer of the page, not copied
const bytes = data => data instanceof Uint8Array ? data : new Uint8Array(data || 0);
const errorText = e => String((e && (e.message || e.name || e.code)) || e);

// ---- output

function addOut(fd, bytes) {
  const text = decoders[fd].decode(bytes, { stream: true });
  if (!text) return;
  const last = out[out.length - 1];
  if (last && last.fd === fd) last.text += text; else out.push({ fd, text });
  outLen += text.length;
  if (outLen > COALESCE) flushOut();
}

function flushOut() {
  for (const { fd, text } of out) {
    unacked += text.length;
    post({ type: 'output', fd, text });
  }
  out = [];
  outLen = 0;
}

function onState(st) {
  flushOut();
  if (upload && (st === WAITING || (mode === 'world' && st === SLEEPING))) keepUpload();
  if (st === WAITING || st === SLEEPING) {
    settle();
    if (mode === 'console') noteEdited(st);
  }
  if (st !== lastState || stateDirty) {
    lastState = st;
    stateDirty = false;
    post({ type: 'state', state: st, waiting: st === WAITING });
  } else if (st === BUSY && performance.now() - lastPostAt >= TICK_MS) {
    post({ type: 'tick' });
  }
}

// ---- storage

// One storage operation after the other, so that a slot is written whole
// before the next one is, and "exit" waits for them
function enqueue(f) {
  storing = storing.then(f).catch(e => console.warn('vm-worker: ' + errorText(e)));
  return storing;
}

function storageFailure(e) {
  const name = e && e.name;
  const why = name === 'QuotaExceededError' ? 'this browser has no room for it'
        : errorText(e);
  return 'the image could not be kept in this browser (' + why + '); use Download to keep a copy';
}

// The slot is another page's now (or none): its .changes is no longer this
// VM's to store
function superseded() {
  owned = null;
  stopSync();
  post({ type: 'superseded' });
}

// The size and time of the .changes of the VM, or ''
function changesKey() {
  try {
    const st = drv.FS.stat(changesPath);
    return st.size + ':' + Number(st.mtime);
  } catch (e) { return ''; }
}

// The Console: whether the VM changed code since it first waited for input
// or its image was last stored, said when it changes
function noteEdited(st) {
  const key = changesKey();
  if (editedBase === null) {
    if (st !== WAITING) return;         // still booting
    editedBase = key;
  }
  const now = key !== editedBase;
  if (now === edited) return;
  edited = now;
  post({ type: 'edited', edited });
}
function storedChanges(key) {
  if (mode !== 'console' || editedBase === null) return;
  editedBase = key;
  noteEdited();
}

// Whether an image has the OSWindow-Web package: the name of its class is
// a ByteSymbol of the image, so its bytes are in the file
function hasBytes(data, text) {
  const t = new TextEncoder().encode(text), last = data.length - t.length;
  for (let i = data.indexOf(t[0]); i >= 0 && i <= last; i = data.indexOf(t[0], i + 1)) {
    let k = 1;
    while (k < t.length && data[i + k] === t[k]) k++;
    if (k === t.length) return true;
  }
  return false;
}
// What the REPL wrote in WORLD_FILE, true or false, or null
function worldSaid() {
  if (!drv || mode !== 'console') return null;
  try {
    const said = new TextDecoder().decode(drv.FS.readFile(WORLD_FILE)).trim();
    return said === 'true' ? true : said === 'false' ? false : null;
  } catch (e) { return null; }
}
// Whether the image can open the world: what the REPL said, once it runs,
// else whether the name of the class is in its bytes, which a string or a
// symbol of that name kept by an image without the package fakes
function isPrepared(image) {
  if (!manifest || !manifest.world) return false;
  const said = worldSaid();
  return said === null ? hasBytes(image, WORLD_CLASS) : said;
}

function startSync() {
  if (!syncTimer) syncTimer = setInterval(() => syncChanges(false), SYNC_MS);
}

function stopSync() {
  clearInterval(syncTimer);
  clearTimeout(settleTimer);
  syncTimer = settleTimer = 0;
}

// The VM went idle: store the .changes soon, if it changed, so that little
// is left to store when the page goes away
function settle() {
  if (!owned || settleTimer) return;
  settleTimer = setTimeout(() => { settleTimer = 0; syncChanges(true); }, SETTLE_MS);
}

// Store the .changes of the slot again, when it changed and, unless forced,
// SYNC_MS passed since it last was
function syncChanges(force) {
  if (!store || !owned || !drv) return storing;
  let key, changes;
  try {
    key = changesKey();
    if (!key || key === syncedKey || (!force && performance.now() - syncedAt < SYNC_MS)) return storing;
    changes = drv.FS.readFile(changesPath);
  } catch (e) { return storing; }
  syncedKey = key;
  syncedAt = performance.now();
  return enqueue(async () => {
    if (!owned) return;
    const meta = await store.syncChanges(changes, owned);
    if (meta) owned = meta; else superseded();
  });
}

function remember(path) {
  imagePath = path;
  changesPath = changesOf(path);
  syncedKey = changesKey();
  syncedAt = performance.now();
}

// The upload booted: the REPL waits for input, the world idles.  Store it
// as the slot, so that the next boot is of it too.  The REPL has now said
// whether it can open the world.
function keepUpload() {
  const { image, changes } = upload;
  const prepared = isPrepared(image);
  upload = null;
  post({ type: 'storing' });
  enqueue(async () => {
    try {
      owned = await store.save(image, changes, { build: manifest.build, prepared });
      startSync();
      post({ type: 'saved', bytes: image.length, prepared, upload: true });
    } catch (e) {
      post({ type: 'saved', bytes: image.length, prepared, upload: true, error: storageFailure(e) });
    }
  });
}

// HOST_IMAGE_SAVED: the VM wrote the image at path.  Read it now, between
// slices, with its .changes, and store both.
function imageSaved(path) {
  let image, changes;
  try {
    image = drv.FS.readFile(path);
    changes = drv.FS.readFile(changesOf(path));
  } catch (e) {
    post({ type: 'saved', bytes: 0, error: 'the saved image cannot be read: ' + errorText(e) });
    return;
  }
  remember(path);
  const key = syncedKey;
  upload = null;                        // this save is the slot now
  const prepared = isPrepared(image);
  if (!store) {
    post({ type: 'saved', bytes: image.length, prepared,
           error: storageError || 'this page does not keep images; use Download to keep a copy' });
    return;
  }
  post({ type: 'storing' });
  enqueue(async () => {
    try {
      owned = await store.save(image, changes, { build: manifest.build, prepared });
      startSync();
      post({ type: 'saved', bytes: image.length, prepared });
      storedChanges(key);
    } catch (e) {
      post({ type: 'saved', bytes: image.length, prepared, error: storageFailure(e) });
    }
  });
}

// ---- loading

async function fetchManifest(url) {
  const r = await fetch(url);
  if (!r.ok) throw new Error(url.replace(/\?.*/, '') + ': HTTP ' + r.status);
  return r.json();
}

// The file f of the manifest, inflated into a buffer of its size
async function fetchFile(f, v, onBytes) {
  const r = await fetch(f.url + v);
  if (!r.ok) throw new Error(f.url + ': HTTP ' + r.status);
  const reader = r.body.getReader();
  // the first two bytes say whether it is still gzipped
  let head = new Uint8Array(0);
  while (head.length < 2) {
    const { done, value } = await reader.read();
    if (done) break;
    const more = new Uint8Array(head.length + value.length);
    more.set(head);
    more.set(value, head.length);
    head = more;
  }
  let body = new ReadableStream({
    start(c) { if (head.length) c.enqueue(head); },
    async pull(c) {
      const { done, value } = await reader.read();
      if (done) c.close(); else c.enqueue(value);
    },
    cancel(reason) { return reader.cancel(reason); },
  });
  if (head[0] === 0x1f && head[1] === 0x8b) body = body.pipeThrough(new DecompressionStream('gzip'));
  const data = new Uint8Array(f.size), inflated = body.getReader();
  let at = 0;
  for (;;) {
    const { done, value } = await inflated.read();
    if (done) break;
    if (at + value.length > data.length) {
      inflated.cancel();
      throw new Error(f.url + ': more than the ' + f.size + ' bytes of manifest.json');
    }
    data.set(value, at);
    at += value.length;
    onBytes(value.length);
  }
  if (at !== data.length) throw new Error(f.url + ': ' + at + ' bytes instead of ' + f.size);
  return data;
}

async function fetchText(url) {
  const r = await fetch(url);
  if (!r.ok) throw new Error(url.replace(/\?.*/, '') + ': HTTP ' + r.status);
  return new Uint8Array(await r.arrayBuffer());
}

// A reporter of progress: report(loaded) posts it, at most every 1/200 of
// the total
function progress(phase, total) {
  let shown = -1;
  const step = Math.max(1, Math.floor(total / 200));
  const report = loaded => {
    if (loaded - shown >= step || loaded === total) {
      shown = loaded;
      post({ type: 'progress', phase, loaded, total });
    }
  };
  report(0);
  return report;
}

// The files of /pharo and where the image came from
async function load(m) {
  const v = '?v=' + encodeURIComponent(manifest.build);
  const imageName = manifest.image, changesName = changesOf(imageName);
  let image = null, changes = null, source = 'download', savedAt = null, prepared = !!manifest.world;
  if (m.upload) {
    image = bytes(m.upload.image);
    changes = bytes(m.upload.changes);
    source = 'upload';
    prepared = isPrepared(image);
  } else if (store) {
    try {
      let report = null;
      const saved = await store.load((loaded, total) => (report || (report = progress('restore', total)))(loaded));
      if (saved) {
        image = saved.image;
        changes = saved.changes;
        source = 'saved';
        savedAt = saved.meta.savedAt;
        prepared = saved.meta.prepared === undefined ? isPrepared(image) : !!saved.meta.prepared;
        owned = saved.meta;
      }
    } catch (e) {
      storageError = e && e.unavailable
        ? 'this browser does not keep images here (' + errorText(e) + '); use Download to keep a copy'
        : 'the saved image could not be read (' + errorText(e) + ')';
    }
  }
  const wanted = manifest.files.filter(f => !image || (f.path !== imageName && f.path !== changesName));
  const report = progress('fetch', wanted.reduce((n, f) => n + f.size, 0));
  let loaded = 0;
  const [fetched, st] = await Promise.all([
    Promise.all(wanted.map(f => fetchFile(f, v, n => report(loaded += n)))),
    Promise.all((manifest.st || []).map(p => fetchText(p + v))),
  ]);
  const files = wanted.map((f, i) => ({ path: DIR + '/' + f.path, data: fetched[i] }));
  (manifest.st || []).forEach((p, i) => files.push({ path: DIR + '/' + p, data: st[i] }));
  if (image) {
    files.push({ path: DIR + '/' + imageName, data: image });
    files.push({ path: DIR + '/' + changesName, data: changes });
  }
  // An upload becomes the slot once it has booted (keepUpload): a broken
  // one must not replace the image saved in this browser
  if (m.upload && store) upload = { image, changes, prepared };
  // Whether an image is saved in this browser: the slot booted, or the one
  // that an upload replaces once it has booted, or one that could not be
  // read (which resetStorage deletes)
  let stored = !!owned;
  if (!stored && store) {
    try { stored = !!(await store.meta()); } catch (e) { /* none that can be used */ }
  }
  return { files, source, savedAt, prepared, stored };
}

async function boot(m) {
  // first: resetStorage needs it even when the rest fails
  if (m.persist) {
    try { store = PharoStorage.open(); } catch (e) { storageError = errorText(e); }
  }
  manifest = await fetchManifest(m.manifestUrl || 'manifest.json' + q);
  if (m.build && m.build !== manifest.build)
    console.warn('vm-worker: the page is of build ' + m.build + ', the files of build ' + manifest.build);
  const { files, source, savedAt, prepared, stored } = await load(m);
  post({ type: 'progress', phase: 'boot', loaded: 0, total: 1 });
  let config;
  if (m.display) {
    importScripts('display-worker.js' + q);
    display = PharoDisplay.create(m.display, post);
    config = { webDisplay: display };
  }
  const image = DIR + '/' + manifest.image;
  const env = mode === 'console' ? { PHARO_WEB_WORLD_FILE: WORLD_FILE } : {};
  if (m.sliceMs > 0) env.PHARO_WASM_SLICE_MS = String(m.sliceMs);
  const options = {
    args: PharoVMDriver.vmArgs(mode, image),
    files,
    cwd: DIR,
    thisProgram: DIR + '/pharo',
    env,
    config,
    wasmModule: m.wasmModule,
    locateFile: (p, dir) => dir + p + q,
    schedule, later,
    canRun: () => unacked < HIGH,
    onOutput: addOut,
    onState,
    onHost: (kind, text) => { if (kind === HOST_IMAGE_SAVED) imageSaved(text); },
    onExit: code => {
      flushOut();
      stopSync();
      syncChanges(true).then(() => post({ type: 'exit', code }));
    },
    onCrash: (message, stack, stacks) => {
      flushOut();
      stopSync();
      storing.then(() => post({ type: 'crash', message, stack, stacks }));
    },
    onDiag: t => console.warn(t.replace(/\n$/, '')),
  };
  drv = await PharoVMDriver.start(createPharoVM, options);
  options.files = null;                 // in MEMFS now
  if (!drv) return;                     // the crash is reported
  // the display paints from the VM's memory: let it know the VM before any
  // slice, whatever the page sends first
  if (display && display.handle) display.handle({ kind: 'attach' }, drv);
  remember(image);
  if (owned) startSync();
  post({ type: 'ready', image, source, persisted: stored, savedAt, world: !!manifest.world, prepared,
         storageError });
  drv.begin();
}

// ---- the messages of the page

function fsOp(m, f) {
  try {
    if (!drv) throw new Error('the VM is not running');
    f(drv.FS);
  } catch (e) {
    post({ type: 'error', id: m.id, message: errorText(e) });
  }
}

function handle(m) {
  const live = drv && !drv.dead;
  switch (m.type) {
  case 'input':
    if (live) { drv.feed(m.text); stateDirty = true; }
    break;
  case 'eof':
    if (live) { drv.eof(); stateDirty = true; }
    break;
  case 'interrupt':
    stateDirty = true;
    post({ type: 'interrupted', registered: live ? drv.interrupt() : false });
    break;
  case 'save':
    if (live && mode !== 'world') { drv.feed(SAVE + '\n'); stateDirty = true; }
    break;
  case 'ack':
    unacked = Math.max(0, unacked - (m.chars || 0));
    if (unacked < HIGH / 2 && live) drv.resumeOutput();
    break;
  case 'fs':
    fsOp(m, FS => {
      switch (m.op) {
      case 'readFile': {
        const data = FS.readFile(m.path);
        post({ type: 'fs-result', id: m.id, path: m.path, data }, [data.buffer]);
        break;
      }
      case 'writeFile':
        FS.writeFile(m.path, typeof m.data === 'string' ? m.data : bytes(m.data));
        post({ type: 'fs-result', id: m.id, path: m.path });
        break;
      case 'listDir':
        post({ type: 'fs-result', id: m.id, path: m.path,
               entries: FS.readdir(m.path).filter(e => e !== '.' && e !== '..') });
        break;
      case 'remove':
        if (FS.isDir(FS.stat(m.path).mode)) FS.rmdir(m.path); else FS.unlink(m.path);
        post({ type: 'fs-result', id: m.id, path: m.path });
        break;
      default:
        throw new Error('unknown fs operation ' + m.op);
      }
    });
    break;
  case 'download':
    fsOp(m, FS => {
      const files = [imagePath, changesPath].map(p => ({ name: p.slice(p.lastIndexOf('/') + 1), data: FS.readFile(p) }));
      for (const { name, data } of files)
        post({ type: 'file', id: m.id, name, data, count: files.length }, [data.buffer]);
    });
    break;
  case 'resetStorage':
    upload = null;                      // nor the upload
    enqueue(async () => {
      stopSync();
      owned = null;
      try {
        if (store) await store.reset();
        else if (storageError) throw new Error(storageError);
        post({ type: 'reset', id: m.id });
      } catch (e) {
        post({ type: 'reset', id: m.id, error: errorText(e) });
      }
    });
    break;
  case 'flush':
    syncChanges(true);
    break;
  case 'display':
    if (display && live) PharoDisplay.onMessage(m, drv);
    break;
  }
}

function replay() {
  settled = true;
  const pending = queue;
  queue = [];
  for (const m of pending) handle(m);
}

onmessage = ({ data: m }) => {
  if (m.type !== 'init') {
    if (settled) handle(m); else queue.push(m);
    return;
  }
  if (mode) return;                     // one VM per worker
  mode = m.mode === 'world' ? 'world' : 'console';
  boot(m).catch(e => {
    flushOut();
    post({ type: 'crash', message: 'the VM could not be loaded: ' + errorText(e), stack: String((e && e.stack) || '') });
  }).then(replay);
};
