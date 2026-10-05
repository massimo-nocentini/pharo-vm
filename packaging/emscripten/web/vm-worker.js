// vm-worker.js - run the Pharo VM for WebAssembly in a Web Worker
//
// The page owns one worker per VM: page.js (the Console) and, in M2,
// world.js (the world).  Messages:
//
//   page -> worker   init {wasmModule, manifestUrl, build, mode, sliceMs,
//                          persist, upload: {image, changes, sources},
//                          prepare, display},
//                    input {text}, eof, interrupt, save, ack {chars},
//                    fs {id, op, path, data}, download {id},
//                    resetStorage {id}, flush, display {...}
//   worker -> page   progress {phase, loaded, total},
//                    ready {image, source, persisted, savedAt, world,
//                           prepared, webPackage, preparing, sources,
//                           storageError, fonts},
//                    output {fd, text}, state {state, waiting}, tick,
//                    interrupted {registered}, edited {edited},
//                    storing, saved {bytes, prepared, webPackage, error,
//                                    upload},
//                    prepared {error, saved, webPackage},
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
// like the st files, which always do.  So does the .sources, unless the
// image comes with one: init.upload {image, changes, sources: {name, data}}
// (Uint8Arrays, ArrayBuffers or Blobs, as open-image.js gives them; sources
// is optional), or the slot, which keeps the .sources of its image when
// that is its own, not the one of the manifest (another name, or size).
// The .sources goes into /pharo under its name, which is the name that the
// image looks for, and ready.sources says which one the VM got.  Every
// file of the manifest is fetched and inflated as it comes
// (DecompressionStream) into a buffer of its size, unless its first bytes
// are not gzip's 1f 8b: a server that sent it with Content-Encoding: gzip
// had the browser inflate it.  Everything goes into /pharo, the working
// directory and the VM's directory (thisProgram is /pharo/pharo), which
// must be writable.  The VM then boots with PharoVMDriver.vmArgs(init.mode):
// the REPL of st/web-repl.st, or the world.  progress says how far the
// loading got: "fetch" (the bytes of the files, inflated), "restore" (the
// slot), then "boot".
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
// from /pharo, says "storing", stores them as the new slot, with the
// .sources of the image when it has its own, and says "saved".
// The slot says which version of OSWindow-Web its image has (webPackage, 0
// without the package), and so whether it can open the world (prepared):
// the version must be at least manifest.webPackage, the one of this site
// (stage.mjs reads it from OSWebDriver class>>packageVersion; a manifest
// without it asks for 1, any OSWindow-Web).  In the Console the REPL tells,
// in the file WORLD_FILE (st/web-repl.st); else the bytes of the image do:
// without the name of the class of OSWindow-Web, 0; else the highest version
// that ends a Symbol #OSWindowWebPackage<version> in them (OSWebDriver class
// >>packageMarker), or 1 when none does (the package before the versions).
// The version is measured from the image, never taken from the manifest; a
// slot saved before there were versions counts as 1 when it was prepared
// (PharoStorage.webPackage).  An image saved in the Console with an older
// version keeps it, and is prepared again on its next world boot.  An
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
// Preparation.  The world page sends init.prepare: an image that cannot
// open the world (not prepared: it lacks OSWindow-Web, or has an older
// version of it) is then prepared for it, as the Console's "Prepare for the
// world" does it.  The worker boots it as the Console would, with the REPL
// and without a display, and says ready {preparing: true}.  Once the REPL
// waits for input (an upload is then kept, as in the Console), the worker
// types PREPARE, which files in st/web-bootstrap.st (OSWindow-Web, and the
// fonts) and saves, in one evaluation that an error stops before the save.
// "prepared" ends it: without error once the image saved, with the version
// of OSWindow-Web of the manifest exactly, as the REPL says, is stored as
// the slot, which the page then boots in a world worker of its own; else
// error says why, with saved: true when the image saved itself (Download
// gives it) but could not be stored or has another version (webPackage,
// stored as it is: the worker does not prepare it again by itself), and
// saved: false when the evaluation failed (error is then what it wrote on
// stderr).  ready.fonts is manifest.fonts, the fonts that a preparation sets
// up: 'freetype' or 'bitmap' (null for a manifest without it).
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
// What OSWebDriver class>>packageMarker answers, a Symbol of the image,
// before the version of the package
const PACKAGE_MARKER = 'OSWindowWebPackage';
// Where the REPL says which version of it its image has, 0 without it
// (PHARO_WEB_WORLD_FILE)
const WORLD_FILE = DIR + '/.pharo-web-world';
// What prepares an image for the world, in one evaluation of the REPL
const PREPARE = "CodeImporter evaluateFileNamed: '" + DIR + "/st/web-bootstrap.st'. " + SAVE;

let drv = null, settled = false, queue = [];
let mode = null, manifest = null, display = null;
let store = null, storageError = null;
// the meta of the slot that this VM booted from or saved, while it is the
// stored one
let owned = null, syncTimer = 0, syncedKey = '', syncedAt = 0, settleTimer = 0;
// the .changes when the VM first waited for input or its image was stored
// (null before), and whether it differs now ("edited")
let editedBase = null, edited = false;
// {image, changes}: the upload that this VM booted, not stored yet
let upload = null;
// {name, data (a Blob)}: the .sources of the image when it is its own, kept
// in the slot with it
let ownSources = null;
// the preparation (init.prepare): 0 none, 1 the REPL boots, 2 it evaluates
// PREPARE, 3 over; what it wrote on stderr meanwhile, and whether it saved
let preparing = 0, prepareErr = '', prepareSaved = false;
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
// and those of a Blob too, read
const read = async data => data instanceof Blob ? new Uint8Array(await data.arrayBuffer()) : bytes(data);
const errorText = e => String((e && (e.message || e.name || e.code)) || e);

// ---- output

function addOut(fd, bytes) {
  const text = decoders[fd].decode(bytes, { stream: true });
  if (!text) return;
  if (preparing === 2 && fd === 2 && prepareErr.length < 4096) prepareErr += text;
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
  if (preparing && st === WAITING) prepareStep();
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

// Each place where the ASCII text is in data, from the first: f gets the
// offset after it, and answers whether to go on
function eachEnd(data, text, f) {
  const t = new TextEncoder().encode(text), last = data.length - t.length;
  for (let i = data.indexOf(t[0]); i >= 0 && i <= last; i = data.indexOf(t[0], i + 1)) {
    let k = 1;
    while (k < t.length && data[i + k] === t[k]) k++;
    if (k === t.length && !f(i + k)) return;
  }
}
// Whether an image has the OSWindow-Web package: the name of its class is
// a ByteSymbol of the image, so its bytes are in the file
function hasBytes(data, text) {
  let found = false;
  eachEnd(data, text, () => { found = true; return false; });
  return found;
}
// The highest version that follows PACKAGE_MARKER in the bytes of the
// image, or 0 (a Symbol is its bytes, then zeros up to its last word)
function markerOf(data) {
  let best = 0;
  eachEnd(data, PACKAGE_MARKER, end => {
    let v = 0, k = end;
    while (k < data.length && k - end < 6 && data[k] >= 0x30 && data[k] <= 0x39) v = v * 10 + data[k++] - 0x30;
    best = Math.max(best, v);
    return true;
  });
  return best;
}
// The version of OSWindow-Web that the world of this site needs
const wantedPackage = () => (manifest && Number.isInteger(manifest.webPackage) && manifest.webPackage) || 1;
// What the REPL wrote in WORLD_FILE: the version of OSWindow-Web of its
// image, 0 without it, or null
function worldSaid() {
  if (!drv || mode !== 'console') return null;
  try {
    const said = new TextDecoder().decode(drv.FS.readFile(WORLD_FILE)).trim();
    // (true and false: what web-repl.st wrote before the versions)
    return /^\d+$/.test(said) ? Number(said) : said === 'true' ? 1 : said === 'false' ? 0 : null;
  } catch (e) { return null; }
}
// The version of OSWindow-Web of the image: what the REPL said, once it
// runs, else what its bytes say, which a string or a symbol of those names
// kept by an image without the package fakes
function webPackageOf(image) {
  const said = worldSaid();
  if (said !== null) return said;
  if (!hasBytes(image, WORLD_CLASS)) return 0;
  return Math.max(1, markerOf(image));
}
// Whether an image with that version of OSWindow-Web can open the world
const canOpen = version => !!(manifest && manifest.world) && version >= wantedPackage();

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
  const webPackage = webPackageOf(image), prepared = canOpen(webPackage);
  upload = null;
  post({ type: 'storing' });
  enqueue(async () => {
    try {
      owned = await store.save(image, changes, { build: manifest.build, prepared, webPackage }, ownSources, null);
      startSync();
      post({ type: 'saved', bytes: image.length, prepared, webPackage, upload: true });
    } catch (e) {
      post({ type: 'saved', bytes: image.length, prepared, webPackage, upload: true, error: storageFailure(e) });
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
  const webPackage = webPackageOf(image), prepared = canOpen(webPackage);
  const prepare = preparing === 2;
  if (prepare) prepareSaved = true;
  if (!store) {
    const error = storageError || 'this page does not keep images; use Download to keep a copy';
    post({ type: 'saved', bytes: image.length, prepared, webPackage, error });
    if (prepare) prepareEnd(error, webPackage);
    return;
  }
  post({ type: 'storing' });
  enqueue(async () => {
    try {
      // (a .sources that the slot of this VM has stays as stored)
      owned = await store.save(image, changes, { build: manifest.build, prepared, webPackage }, ownSources, owned);
      startSync();
      post({ type: 'saved', bytes: image.length, prepared, webPackage });
      storedChanges(key);
      if (prepare) prepareEnd(prepareMismatch(webPackage), webPackage);
    } catch (e) {
      post({ type: 'saved', bytes: image.length, prepared, webPackage, error: storageFailure(e) });
      if (prepare) prepareEnd(storageFailure(e), webPackage);
    }
  });
}

// ---- the preparation for the world (init.prepare)

// The REPL waits for input: first type PREPARE; when it waits again and
// the evaluation did not save, the preparation failed
function prepareStep() {
  if (preparing === 1) {
    preparing = 2;
    drv.feed(PREPARE + '\n');
    stateDirty = true;
  } else if (preparing === 2 && !prepareSaved) {
    preparing = 3;
    post({ type: 'prepared', saved: false,
           error: prepareErr.trim() || 'web-bootstrap.st did not save the image' });
  }
}
// What is wrong with the version of OSWindow-Web of the image that PREPARE
// saved, or null: it must be the one of the manifest, exactly
function prepareMismatch(version) {
  const wanted = wantedPackage();
  if (version === wanted && canOpen(version)) return null;
  if (!version) return 'the image still cannot open the world after web-bootstrap.st';
  return 'web-bootstrap.st left version ' + version + ' of OSWindow-Web in the image, but this site has version ' +
    wanted;
}
// The image that PREPARE saved, with that version of OSWindow-Web, is
// stored (error null), or not
function prepareEnd(error, webPackage) {
  preparing = 3;
  post(error ? { type: 'prepared', saved: true, webPackage, error } : { type: 'prepared', saved: true, webPackage });
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
  const siteSources = manifest.files.find(f => /\.sources$/.test(f.path)) || null;
  let image = null, changes = null, source = 'download', savedAt = null;
  // the image of the site has the version of the manifest: stage.mjs reads
  // it from the package that prepared that image
  let webPackage = manifest.world ? wantedPackage() : 0;
  let sources = null;                   // {name, data}: the .sources that came with the image
  if (m.upload) {
    image = await read(m.upload.image);
    changes = await read(m.upload.changes);
    if (m.upload.sources) {
      const name = String(m.upload.sources.name || '');
      if (!/^[^/]+\.sources$/.test(name)) throw new Error('the .sources of the upload is named ' + JSON.stringify(name));
      sources = { name, data: m.upload.sources.data };
    }
    source = 'upload';
    webPackage = webPackageOf(image);
  } else if (store) {
    try {
      let report = null;
      const saved = await store.load((loaded, total) => (report || (report = progress('restore', total)))(loaded));
      if (saved) {
        image = saved.image;
        changes = saved.changes;
        sources = saved.sources;
        source = 'saved';
        savedAt = saved.meta.savedAt;
        const kept = PharoStorage.webPackage(saved.meta);
        webPackage = kept === null ? webPackageOf(image) : kept;
        owned = saved.meta;
      }
    } catch (e) {
      storageError = e && e.unavailable
        ? 'this browser does not keep images here (' + errorText(e) + '); use Download to keep a copy'
        : 'the saved image could not be read (' + errorText(e) + ')';
    }
  }
  // the .sources of the image is its own unless it is the one of the
  // manifest, which is then not fetched either
  let sourcesData = null;
  if (sources) {
    sourcesData = await read(sources.data);
    if (!siteSources || sources.name !== siteSources.path || sourcesData.length !== siteSources.size)
      ownSources = { name: sources.name, data: sources.data instanceof Blob ? sources.data : new Blob([sourcesData]) };
  }
  const wanted = manifest.files.filter(f => (!image || (f.path !== imageName && f.path !== changesName)) &&
                                       !(sourcesData && f === siteSources));
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
  if (sourcesData) files.push({ path: DIR + '/' + sources.name, data: sourcesData });
  // An upload becomes the slot once it has booted (keepUpload): a broken
  // one must not replace the image saved in this browser
  if (m.upload && store) upload = { image, changes };
  // Whether an image is saved in this browser: the slot booted, or the one
  // that an upload replaces once it has booted, or one that could not be
  // read (which resetStorage deletes)
  let stored = !!owned;
  if (!stored && store) {
    try { stored = !!(await store.meta()); } catch (e) { /* none that can be used */ }
  }
  return { files, source, savedAt, webPackage, prepared: canOpen(webPackage), stored,
           sources: sourcesData ? sources.name : siteSources ? siteSources.path : null };
}

async function boot(m) {
  // first: resetStorage needs it even when the rest fails
  if (m.persist) {
    try { store = PharoStorage.open(); } catch (e) { storageError = errorText(e); }
  }
  manifest = await fetchManifest(m.manifestUrl || 'manifest.json' + q);
  if (m.build && m.build !== manifest.build)
    console.warn('vm-worker: the page is of build ' + m.build + ', the files of build ' + manifest.build);
  const { files, source, savedAt, webPackage, prepared, stored, sources } = await load(m);
  // an image that cannot open the world is prepared for it, in the REPL: one
  // without OSWindow-Web, or with an older version of it
  if (m.prepare && mode === 'world' && manifest.world && !prepared) {
    preparing = 1;
    mode = 'console';
  }
  post({ type: 'progress', phase: 'boot', loaded: 0, total: 1 });
  let config;
  if (m.display && !preparing) {
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
  post({ type: 'ready', image, source, persisted: stored, savedAt, world: !!manifest.world, prepared, webPackage,
         preparing: !!preparing, sources, storageError, fonts: manifest.fonts || null });
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
