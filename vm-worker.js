// vm-worker.js - run the Pharo VM for WebAssembly in a Web Worker
//
// The page owns one worker per VM: page.js (the Console), notebook.js (the
// notebook kernel of the Console page, through nb-kernel.js) and, in M2,
// world.js (the world).
// Messages:
//
//   page -> worker   init {wasmModule, manifestUrl, build, mode, sliceMs,
//                          persist, upload: {name, image, changes, sources},
//                          slot, fresh, name, prepare, display, gitProxy},
//                    input {text}, eof, interrupt, save, saveAs {name},
//                    ack {chars}, fs {id, op, path, ...}, download {id},
//                    resetStorage {id, slot, all}, flush, display {...},
//                    gitProxy {gitProxy}
//   worker -> page   progress {phase, loaded, total},
//                    ready {image, source, persisted, savedAt, world,
//                           prepared, webPackage, preparing, sources,
//                           storageError, fonts, git, gitHttp, mode,
//                           slot, imageId, pharo, slotMissing, freshMissing,
//                           sourcesMissing, gitProxy, gitProxyDefault,
//                           gitCredentials},
//                    output {fd, text}, output {fd: 3, bytes},
//                    state {state, waiting}, tick,
//                    interrupted {registered}, edited {edited},
//                    storing, saved {bytes, prepared, webPackage, error,
//                                    upload, slot, created, treeFiles,
//                                    treeSize, treeError},
//                    prepared {error, saved, webPackage, slot},
//                    superseded {slot}, reset {id, slot, error},
//                    exit {code}, crash {message, stack, stacks},
//                    fs-result {id, ...}, file {id, name, data, count},
//                    error {id, message}, display {...},
//                    git {event, method, host, proxy, status, error,
//                         credentials, redirectedTo, proxyRefused}
//
// Messages that arrive before "ready" are queued and replayed after it;
// "ready" always precedes the first "state".  When the VM cannot start, or
// the boot fails, the worker says "crash" instead, and still answers what
// needs no VM (resetStorage, whose storage is opened first of all, so that it
// deletes a slot even when the manifest could not be fetched).  After
// "exit" and "crash" it stays, so that the files of the VM can still be read
// and downloaded, until the page terminates it.  "fs" ops: readFile,
// writeFile {data, mode, mtime} (with the directories it needs), listDir,
// remove, mkdir {mode}, symlink {target} and stat (fs-result {mode, size,
// mtime, link}).
//
// Boot.  init.manifestUrl is web/manifest.json, written by stage.mjs, whose
// catalog (catalog.js, PharoCatalog) lists the images that the site offers.
// The image and its .changes come from, the first that applies:
// init.upload (an image the user opened, as open-image.js gives it); the
// catalog entry init.fresh (an unknown id: the default entry, and
// ready.freshMissing says it); the slot init.slot of vm-storage.js (one
// that is not there: the default entry, and ready.slotMissing says it);
// with init.persist, the slot booted or saved last (PharoStorage last());
// the default entry of the catalog.  An entry's files are its image, its
// .changes and its .sources, fetched with the st files, which always are.
// A slot or an upload brings the image and the .changes, and the .sources
// is: its own (init.upload's sources {name, data}, Uint8Arrays,
// ArrayBuffers or Blobs, as open-image.js gives them, or the one a slot
// keeps); else the one of its name that the site serves (any entry's, or
// manifest.extraSources), the name being the one the meta of the slot says
// (sourcesName) or the one the image's bytes have
// (Pharo<v>-<bits>bit-<commit>, when they have one only); none when the
// site does not serve that one (ready.sourcesMissing names it: the image
// shows its methods decompiled); and the default entry's when no name is
// known.  A .sources of its own that is one of the site (the same name and
// size) is not its own.  The .sources goes into /pharo under its name,
// which is the name that the image looks for, and ready.sources says which
// one the VM got.  Every file of the manifest is fetched and inflated as it
// comes (DecompressionStream) into a buffer of its size, unless its first
// bytes are not gzip's 1f 8b: a server that sent it with Content-Encoding:
// gzip had the browser inflate it.  Everything goes into /pharo, the
// working directory and the VM's directory (thisProgram is /pharo/pharo),
// which must be writable: the files of the slot first (its tree, below),
// then the placeholders of manifest.libraries, empty files
// (libcairo.so.2): the image finds such a library only as a file of the
// VM's directory (FFIUnix64LibraryFinder), and the VM, which has it built
// in, never reads them; then the site's files and the image.  The image is
// always /pharo/Pharo.image.  The VM then boots with
// PharoVMDriver.vmArgs(init.mode): the REPL of st/web-repl.st, or the world
// ('world'), or the notebook kernel of st/web-notebook.st
// ('notebook', below).  progress says how far the loading got: "fetch"
// (the bytes of the files, inflated), "restore" (the slot), then "boot".
// ready.slot is the slot booted, {slot, name, pharo, imageId, origin,
// savedAt}, or null (a fresh image, an upload); ready.imageId and
// ready.pharo say which catalog entry and which Pharo the image is of, as
// far as known.
//
// Output.  What the VM writes to fd 1 and 2 is decoded as UTF-8 (one
// streaming decoder per fd), coalesced up to 64 KB, and posted at the end
// of every slice.  It is credit based: the page acks the characters it
// rendered, and the VM is paused (between slices) while more than 1 MiB are
// unacknowledged, until fewer than 512 KiB are.  The notebook kernel's
// events (fd 3) stay bytes: output {fd: 3, bytes}, a Uint8Array that is
// transferred, coalesced with the fd 3 writes next to it, and posted in the
// order of the writes to fd 1 and 2; its acks count bytes, against the
// same credit.  "state" comes when the state changes or after the page sent
// something; while the VM is BUSY and nothing else was posted for a second,
// "tick" says that it is alive.
//
// Persistence.  The browser keeps any number of images, each in a slot of
// vm-storage.js.  A VM is bound to the slot it saves into: the slot it
// booted from, else none (a fresh image, an upload) until its first save
// makes it one, named init.name, else the label of its catalog entry (Pharo
// 14), else the name of the upload (a name a slot has gets " (2)", " (3)",
// ...); a new slot has an id of its own, never 'main' (vm-storage.js).
// When the VM has written an image (HOST_IMAGE_SAVED, which comes after the
// slice that saved it), the worker reads it, its .changes and the tree of
// /pharo from MEMFS, says "storing", stores them into the slot bound, with
// the .sources of the image when it has its own, and says "saved {slot:
// {slot, name, pharo}, created}": created when the save made a new slot.
// After "saveAs {name}" the next save makes a new slot of that name, which
// the VM is then bound to (the Console types the save itself; the world
// page asks its image).  A slot says what the image is: sourcesName, pharo
// and imageId, and its origin (fresh:<id>, upload:<name>, save-as:<slot>);
// the worker says them for a slot of before (annotate) when it boots it.
// The slot booted or saved is the last one (setLast), which a page told no
// slot boots.
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
// (PharoStorage.webPackage); a fresh image has the version of its catalog
// entry.  An image saved in the Console with an older version keeps it, and
// is prepared again on its next world boot.  An upload becomes a slot, a
// new one, never over another, only once it has booted: when the REPL first
// waits for input (the world: first sleeps), with "saved {upload: true}";
// one that crashes or quits before is not kept.  ready.persisted says
// whether the browser has a slot (one that could not be read too).  The
// .changes of the slot that the worker booted from or saved is stored again
// when it changed: SETTLE_MS after the VM went idle (the REPL waits for
// input, the world sleeps), at the latest SYNC_MS after it was last stored,
// on "flush" (the page goes away, which a worker may not outlive: little is
// left for it), and when the VM exits; "exit" is posted once the storage is
// done.  Only while that slot is still the stored one, though: once another
// page (a second tab, the world) saved over that slot or deleted it, the
// .changes of this VM would not match the image stored, so the worker stops
// and says "superseded {slot}", until its VM saves again (into that slot:
// the latest save wins; a slot deleted is made again, with its name and
// origin).  Unless the id of the slot is another image's now (a createdAt
// that is not the one of the slot bound: no page makes a slot under an id
// that was another's, but a store may be written by others): the save then
// makes a slot of its own, under the name and origin of the one bound, and
// never overwrites the other image.  Pages on other slots never supersede each other.
// A failure of the storage is reported in "saved", never fatal.  "save"
// types Smalltalk snapshot: true andQuit: false into the REPL; the image
// itself may save at any time too.  In the Console, "edited" says whether
// the .changes of the VM differs from the one it had when it first waited
// for input, or when its image was last stored: code was changed that the
// slot does not have.  "resetStorage" deletes a slot: {all: true} every
// one, {slot} that one, else the one bound, else the one that init told the
// VM to boot (init.slot, else, without init.upload or init.fresh, the last
// one: its boot failed, or the slot could not be read), else none: a fresh
// image or an upload not kept deletes nothing ("reset {slot: null}");
// "reset {slot}" says which, and a VM whose slot was deleted is bound to
// none.
//
// The tree.  A save keeps with the image the files of /pharo that are
// neither the image's (a top-level .image, .changes or .sources) nor the
// site's (st/, the placeholders, WORLD_FILE): Iceberg's clones in
// pharo-local with their .git, the package cache, PharoDebug.log, what the
// user wrote; files, directories (empty ones too: git needs them) and
// symbolic links, with their modes and times.  They are read in the same
// pause between slices as the image, so they are what the image knew.  At
// most TREE_MAX_BYTES and TREE_MAX_ENTRIES: beyond, the slot keeps the
// tree it had, and "saved {treeError}" says what is lost on a reload.  So
// it does when the browser refuses the save with the tree (it has no room
// for it: the tree is written in the transaction of the image): the image
// is then saved again without it, so that the files never cost the image.  A
// boot of the slot writes them back into /pharo before anything else (a
// path with '', '.' or '..' in it is skipped); saved {treeFiles, treeSize}
// says what the slot keeps.
//
// The home.  The VM's HOME is HOME, /pharo/home, in the tree: the worker
// makes it at every boot (after the tree of the slot, which may have it),
// in every mode.  So what the image and libgit2 keep in HOME is kept with
// the image, as any file of the tree: above all the global .gitconfig,
// where Iceberg's dialog of the missing properties (the user of a commit)
// writes when its "Save properties as global?" is checked: once the image
// is saved, no commit asks again, after a reload too.  It is the home of
// the slot, not of the browser: each slot keeps its own, as it keeps its
// clones, an image saved as another slot takes a copy, and a fresh image,
// an upload and a slot saved before there was a home start with an empty
// one.  The notebook reads the one of the slot it boots.  (Emscripten's own
// HOME, /home/web_user, is in no tree.)  A slot saved before there was a
// home may keep a file or a link named home there, of the user's: the VM
// then keeps Emscripten's HOME, and the worker warns.
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
// of OSWindow-Web of the manifest exactly, as the REPL says, is stored in
// the slot bound, prepared.slot, which the page then boots in a world worker
// of its own; else error says why, with saved: true when the image saved
// itself (Download gives it) but could not be stored or has another version
// (webPackage, stored as it is: the worker does not prepare it again by
// itself), and saved: false when the evaluation failed (error is then what
// it wrote on stderr).  ready.fonts is manifest.fonts, the fonts that a
// preparation sets up: 'freetype' or 'bitmap' (null for a manifest without
// it).
//
// M2.  With init.display the worker loads display-worker.js, whose
// PharoDisplay.create(init.display, post) is the webDisplay of the VM.  It
// hands the display the driver as soon as the VM is created, with the
// message {kind: 'attach'} (which asks for nothing else), and then the
// "display" messages of the page, with PharoDisplay.onMessage.
//
// Git.  init.gitProxy, and later "gitProxy", is the CORS proxy of git's
// HTTP requests, as the user chose it in Settings (git-proxy.js keeps it):
// an http: or https: URL prefix, '' for none, and null (or undefined) for
// the site's default, manifest.gitProxyDefault (stage.mjs, WASM_GIT_PROXY;
// '' or absent: none).  The worker makes it Module.gitHttpProxy, which the
// smart-HTTP transport of libgit2 (src/emscripten/gitSupport.c) reads at
// each request, so a change holds from the next one.  It goes nowhere else:
// not into the environment of the image, nor to any server but the proxy
// itself.  ready.gitProxy is the proxy in effect ('' for none),
// ready.gitProxyDefault the site's.
// The credentials that a host asks for (a push to github.com, a private
// repository: the transport asks Iceberg for them after a 401, and sends
// them in an Authorization header) go through a proxy only when the user
// chose it: Module.gitHttpCredentials is false while the proxy is the
// site's default (gitProxy null, and a default there is), a public proxy
// that would see them, and the transport then refuses them before it asks
// Iceberg; true with a proxy of the user's (My proxy, which may be the
// site's default, chosen there) or none (TLS goes to the host itself).
// ready.gitCredentials says which.
// The transport calls Module.onGitHttp ({method, host, proxy, status,
// error, authorization, retried, credentials, redirectedTo, proxyRefused})
// after every request, and the worker posts "git {event: 'request'}" at the
// first one of its VM, "git {event: 'credentials'}" at the first one that
// carried credentials through a proxy, and "git {event: 'failed'}" when one
// got no answer (status < 0) or an HTTP error (400 and up), at most every
// GIT_FAILED_MS, for the page to say why: never for a 401 that the
// transport answers with credentials (retried), and always when it refused
// to send credentials through the proxy (credentials 'refused'), which the
// page then offers to choose for them, and when a host redirected a request
// with credentials to another host, redirectedTo, through the proxy, which
// sent them there (credentials 'redirected', whatever the status: the
// transport stopped the request).  None of these says what the credentials
// are.
// ready.git says whether the VM has libgit2 (manifest.git, WASM_LIBGIT2),
// ready.gitHttp whether it has that transport (manifest.gitHttp,
// WASM_LIBGIT2_HTTP), which is what the proxy is for.
//
// Notebook.  init.mode 'notebook' boots the notebook kernel: the VM files
// in st/web-notebook.st (vmArgs('notebook')) with the device of its events
// (PharoVMDriver's events), which the worker posts as fd 3 output.  The
// requests are JSON lines that the page types with "input"; Stop is
// "interrupt", as in the Console.  It boots as the Console does (init.slot
// or init.fresh, as the Console's page gives them, else with init.persist
// the last slot, with its tree), but only reads the slot: it never stores
// an image, the .changes or anything else (not even the last slot), and
// posts no storing, saved, edited, superseded or prepared.  The VM's saves
// (HOST_IMAGE_SAVED), save, saveAs, init.upload and init.prepare are
// ignored, and resetStorage answers reset {id, error: 'not in notebook
// mode'}.  ready says mode: 'notebook' (the other modes' ready has no
// mode).
//
// The worker URL's ?v= query (the build id) is passed on to every script.

'use strict';

const V = new URLSearchParams(self.location.search).get('v') || '';
const q = V ? '?v=' + encodeURIComponent(V) : '';
importScripts('pharo-web.js' + q, 'vm-driver.js' + q, 'vm-storage.js' + q, 'catalog.js' + q);

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
// The HOME of the VM, in the tree of the slot (see The home)
const HOME = DIR + '/home';
// What prepares an image for the world, in one evaluation of the REPL
const PREPARE = "CodeImporter evaluateFileNamed: '" + DIR + "/st/web-bootstrap.st'. " + SAVE;
// The most that the tree of a slot keeps: the bytes of its files, and its
// entries
const TREE_MAX_BYTES = 256 * 1024 * 1024, TREE_MAX_ENTRIES = 50000;
// "git {event: 'failed'}" at most this often
const GIT_FAILED_MS = 2000;
const { MAIN, SLOT_ID } = PharoStorage;

let drv = null, settled = false, queue = [];
let mode = null, manifest = null, display = null;
let store = null, storageError = null;
// the slot that this VM saves into (it booted from it or saved it), or null
// (a fresh image or an upload, until its first save), and its name; only
// resetStorage of it unbinds it.  boundSince and boundOrigin are the
// createdAt and the origin of that slot: a slot whose meta has another
// createdAt is another image under the same id, which a save never
// overwrites
let bound = null, boundName = null, boundSince, boundOrigin;
// the slot that init told this VM to boot (init.slot, else, without
// init.upload or init.fresh, the last one), while it is bound to none: the
// one that resetStorage without a slot deletes when the boot failed or the
// slot could not be read; null once the VM is bound, and for a fresh image
// or an upload, which delete nothing
let intended = null;
// the catalog entry that this VM booted from the site, or null; the name
// that init gave for the slot it becomes, and the name of the upload
let freshEntry = null, initName = '', uploadName = '';
// the name of the next save, a new slot (saveAs), or null
let saveAsName = null;
// what the image is: the name of its .sources when known (the one it reads,
// whoever serves it), its version and its catalog id
let sourcesName = null, pharo = null, imageId = null;
// git's requests: whether one was made, whether one carried credentials
// through a proxy, and when one last failed
let gitRequested = false, gitCredentialed = false, gitFailedAt = -Infinity;
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
// the output not posted yet: {fd, text}, or {fd: 3, chunks} (Uint8Arrays)
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
const concat = chunks => {
  const all = new Uint8Array(chunks.reduce((n, c) => n + c.length, 0));
  let at = 0;
  for (const c of chunks) { all.set(c, at); at += c.length; }
  return all;
};
const errorText = e => String((e && (e.message || e.name || e.code)) || e);
// A proxy of git's requests, or ''
const proxyOf = p => typeof p === 'string' && /^https?:\/\//i.test(p) ? p : '';
// The default proxy of the site (manifest.gitProxyDefault), or ''
const defaultProxy = () => proxyOf(manifest && manifest.gitProxyDefault);
// The proxy of git's requests for what the page gave: null or undefined
// for the site's default, else its choice ('' none; the page checked it)
const gitProxyOf = p => p == null ? defaultProxy() : proxyOf(p);
// Whether credentials may go through that proxy: not through the site's
// default (see Git above)
const gitCredentialsOf = p => p != null || !defaultProxy();

// ---- output

function addOut(fd, bytes) {
  if (fd === 3) {
    const last = out[out.length - 1];
    if (last && last.fd === 3) last.chunks.push(bytes); else out.push({ fd, chunks: [bytes] });
    outLen += bytes.length;
    if (outLen > COALESCE) flushOut();
    return;
  }
  const text = decoders[fd].decode(bytes, { stream: true });
  if (!text) return;
  if (preparing === 2 && fd === 2 && prepareErr.length < 4096) prepareErr += text;
  const last = out[out.length - 1];
  if (last && last.fd === fd) last.text += text; else out.push({ fd, text });
  outLen += text.length;
  if (outLen > COALESCE) flushOut();
}

function flushOut() {
  for (const { fd, text, chunks } of out) {
    if (chunks) {
      // every chunk is a buffer of its own (vm-driver.js), which goes as it is
      const data = chunks.length === 1 ? chunks[0] : concat(chunks);
      unacked += data.length;
      post({ type: 'output', fd, bytes: data }, [data.buffer]);
      continue;
    }
    unacked += text.length;
    post({ type: 'output', fd, text });
  }
  out = [];
  outLen = 0;
}

function onState(st) {
  flushOut();
  if (upload && (st === WAITING || (mode !== 'console' && st === SLEEPING))) keepUpload();
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
// VM's to store.  It stays bound: the next save of this VM is its again.
function superseded() {
  owned = null;
  stopSync();
  post({ type: 'superseded', slot: bound });
}

// The slot of meta is this VM's now, under that name: the one to boot when
// a page is told none
async function bind(meta, name) {
  bound = meta.slot || MAIN;
  owned = meta;
  boundSince = meta.createdAt;
  boundOrigin = meta.origin;
  intended = null;
  if (name) boundName = name;
  try { await store.setLast(bound); } catch (e) { /* only a convenience */ }
}

// What "saved" and "ready" say of the slot bound
const slotSaid = () => ({ slot: bound, name: boundName, pharo });

// name, else name (2), name (3), ...: one that no slot has
async function uniqueName(name) {
  let taken = [];
  try { taken = (await store.list()).map(s => s.name); } catch (e) { /* none known */ }
  if (!taken.includes(name)) return name;
  for (let n = 2; ; n++) if (!taken.includes(name + ' (' + n + ')')) return name + ' (' + n + ')';
}

// What a save adds to the meta of its slot (vm-storage.js)
function extraOf(prepared, webPackage, origin, tree) {
  const extra = { build: manifest.build, prepared, webPackage, sourcesName, pharo, imageId, tree };
  if (origin) extra.origin = origin;
  return extra;
}

// ---- the tree of a slot: the files of /pharo but the image's and the site's

// The files of DIR that a save keeps with the image, read now (between
// slices): {tree: {entries, data, size, files}} (null when there is none),
// or {error} when they are too many or too large (TREE_MAX_*), and the slot
// keeps the tree it has.  Not the image, its .changes or a .sources, nor
// what the site gives at every boot (st/, the placeholders of
// manifest.libraries) or the REPL's WORLD_FILE: Iceberg's clones in
// pharo-local, the package cache, PharoDebug.log and what the user wrote.
function captureTree() {
  const FS = drv.FS, entries = [], chunks = [];
  const skip = new Set(['st', WORLD_FILE.slice(DIR.length + 1)].concat(manifest.libraries || []));
  let size = 0, count = 0, over = false;
  const walk = (dir, rel) => {
    let names;
    try { names = FS.readdir(dir); } catch (e) { return; }
    for (const name of names.sort()) {
      if (name === '.' || name === '..') continue;
      if (!rel && (skip.has(name) || /\.(image|changes|sources)$/.test(name))) continue;
      const p = rel ? rel + '/' + name : name, full = dir + '/' + name;
      let st;
      try { st = FS.lstat(full); } catch (e) { continue; }
      const mode = st.mode, mtime = Number(st.mtime) || 0;
      if (FS.isLink(mode)) {
        if (!over) entries.push([p, 'l', mode, mtime, 0, 0, FS.readlink(full)]);
      } else if (FS.isDir(mode)) {
        if (!over) entries.push([p, 'd', mode, mtime, 0, 0, null]);
      } else if (FS.isFile(mode)) {
        if (over) size += st.size;
        else {
          const data = FS.readFile(full);
          entries.push([p, 'f', mode, mtime, size, data.length, null]);
          chunks.push(data);
          size += data.length;
        }
      } else continue;                  // a device, a pipe: not kept
      count++;
      if (!over && (size > TREE_MAX_BYTES || count > TREE_MAX_ENTRIES)) {
        over = true;
        entries.length = chunks.length = 0;
      }
      if (FS.isDir(mode)) walk(full, p);
    }
  };
  walk(DIR, '');
  if (over) {
    return { error: 'the files of the image (' + Math.round(size / 1048576) + ' MB, ' + count +
                    ' files) are too large to keep: clones made since the last save are lost on reload' };
  }
  if (!entries.length) return { tree: null };
  return { tree: { entries, data: new Blob(chunks), size, files: entries.length } };
}

// The files of the tree of a slot, for the driver to write into DIR (a
// path that is not one is said and skipped)
function treeFiles(tree) {
  const files = [];
  for (const e of tree.entries || []) {
    const [p, kind, mode, mtime, offset, length, link] = Array.isArray(e) ? e : [];
    const path = DIR + '/' + p;
    if (!PharoStorage.checkPath(p)) {
      console.warn('vm-worker: a file of the saved image is not restored: ' + JSON.stringify(p));
    } else if (kind === 'd') {
      files.push({ path, dir: true, mode });
    } else if (kind === 'f' && Number.isInteger(offset) && Number.isInteger(length) && offset >= 0 &&
               offset + length <= tree.data.length) {
      files.push({ path, data: tree.data.subarray(offset, offset + length), mode, mtime });
    } else if (kind === 'l' && typeof link === 'string') {
      files.push({ path, link });
    } else {
      console.warn('vm-worker: a file of the saved image is damaged: ' + JSON.stringify(p));
    }
  }
  return files;
}

// ---- the .sources

// The name of the .sources that the image reads, from its bytes: every
// Pharo<major>.<minor>-<32|64>bit-<7 hex digits> in them, not followed by
// another hex digit, when they are all one; else null
function sourcesNameOf(image) {
  const names = new Set();
  const digits = (k, f) => { const s = k; while (k < image.length && f(image[k])) k++; return k > s ? k : -1; };
  const isDigit = c => c >= 0x30 && c <= 0x39, isHex = c => isDigit(c) || (c >= 0x61 && c <= 0x66);
  eachEnd(image, 'Pharo', end => {
    let k = digits(end, isDigit);
    if (k < 0 || image[k] !== 0x2e) return true;
    k = digits(k + 1, isDigit);
    if (k < 0 || image[k] !== 0x2d) return true;
    const bits = String.fromCharCode(image[k + 1], image[k + 2]) + String.fromCharCode(...image.subarray(k + 3, k + 7));
    if (bits !== '64bit-' && bits !== '32bit-') return true;
    const h = k + 7;
    for (let i = h; i < h + 7; i++) if (!(i < image.length && isHex(image[i]))) return true;
    if (h + 7 < image.length && isHex(image[h + 7])) return true;
    names.add(new TextDecoder().decode(image.subarray(end - 5, h + 7)));
    return names.size < 2;
  });
  return names.size === 1 ? [...names][0] + '.sources' : null;
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

// The upload booted: the REPL waits for input, the world idles.  Keep it
// in a new slot, never over another one, so that the next boot is of it
// too.  The REPL has now said whether it can open the world.
function keepUpload() {
  const { image, changes } = upload;
  const webPackage = webPackageOf(image), prepared = canOpen(webPackage);
  upload = null;
  post({ type: 'storing' });
  enqueue(async () => {
    const said = { type: 'saved', bytes: image.length, prepared, webPackage, upload: true };
    try {
      const name = await uniqueName(initName || uploadName || 'Opened image');
      const meta = await store.save(image, changes, extraOf(prepared, webPackage, 'upload:' + (uploadName || name)),
                                    ownSources, null, { slot: null, name });
      await bind(meta, name);
      startSync();
      post(Object.assign(said, { slot: slotSaid(), created: true, treeFiles: 0, treeSize: 0 }));
    } catch (e) {
      post(Object.assign(said, { error: storageFailure(e) }));
    }
  });
}

// HOST_IMAGE_SAVED: the VM wrote the image at path.  Read it now, between
// slices, with its .changes and the tree of DIR, and store them: into the
// slot bound, else (a fresh image, an upload not kept yet, or saveAs) into
// a new slot, which is bound from then on.
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
  const wasUpload = !!upload;
  upload = null;                        // this save is the slot now
  const asName = saveAsName;
  saveAsName = null;
  const webPackage = webPackageOf(image), prepared = canOpen(webPackage);
  const prepare = preparing === 2;
  if (prepare) prepareSaved = true;
  if (!store) {
    const error = storageError || 'this page does not keep images; use Download to keep a copy';
    post({ type: 'saved', bytes: image.length, prepared, webPackage, error });
    if (prepare) prepareEnd(error, webPackage);
    return;
  }
  let tree;
  try { tree = captureTree(); }
  catch (e) { tree = { error: 'the files of the image cannot be read: ' + errorText(e) }; }
  post({ type: 'storing' });
  enqueue(async () => {
    const said = { type: 'saved', bytes: image.length, prepared, webPackage };
    if (tree.error) said.treeError = tree.error;
    try {
      let target, origin, name = null, recreated = false;
      // the slot bound as it is stored now: when another image took its id
      // meanwhile (another createdAt), this one goes into a slot of its own,
      // under its name and origin; when it was deleted, the save makes it
      // again, with them.  (A page could still take the id between this
      // read and the write: a save is a few ms, a takeover a user's doing.)
      const stored = bound && !asName ? await store.meta(bound) : null;
      const taken = !!stored && stored.createdAt !== boundSince;
      if (asName || !bound || taken) {
        name = await uniqueName(asName || (taken && boundName) || initName || (wasUpload && uploadName) ||
                                (freshEntry && freshEntry.label) || (taken && PharoCatalog.labelOf(pharo)) ||
                                'Saved image');
        origin = taken ? boundOrigin : asName && bound ? 'save-as:' + bound : wasUpload ? 'upload:' + (uploadName || name)
          : freshEntry ? 'fresh:' + freshEntry.id : undefined;
        target = { slot: null, name };
      } else {
        recreated = !stored;
        if (recreated) origin = boundOrigin;
        target = { slot: bound };
      }
      // (a .sources that the slot of this VM has stays as stored; a new slot
      // gets the one of this VM written)
      let meta;
      try {
        meta = await store.save(image, changes, extraOf(prepared, webPackage, origin, tree.tree), ownSources, owned,
                                target);
      } catch (e) {
        // the files may be what the browser has no room for: the image is
        // kept without them (the slot keeps the tree it had), as when they
        // are too large
        if (!tree.tree || (e && e.unavailable)) throw e;
        meta = await store.save(image, changes, extraOf(prepared, webPackage, origin, undefined), ownSources, owned,
                                target);
        said.treeError = 'the files of the image (' + Math.round(tree.tree.size / 1048576) + ' MB, ' +
          tree.tree.files + ' files) could not be kept with it (' +
          (e && e.name === 'QuotaExceededError' ? 'this browser has no room for them' : errorText(e)) +
          '): clones made since the last save are lost on reload';
      }
      if (recreated && boundName) {
        try { await store.rename(meta.slot || MAIN, boundName); } catch (e) { /* the one known */ }
      }
      if (!name) {
        try { name = await store.name(meta.slot || MAIN); } catch (e) { /* the one known */ }
      }
      await bind(meta, name || boundName || PharoCatalog.labelOf(pharo));
      startSync();
      post(Object.assign(said, { slot: slotSaid(), created: !target.slot, treeFiles: meta.tree ? meta.treeFiles : 0,
                                 treeSize: meta.tree ? meta.treeSize : 0 }));
      storedChanges(key);
      if (prepare) prepareEnd(prepareMismatch(webPackage), webPackage);
    } catch (e) {
      post(Object.assign(said, { error: storageFailure(e) }));
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
  const said = { type: 'prepared', saved: true, webPackage, slot: bound };
  post(error ? Object.assign(said, { error }) : said);
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

// The files of /pharo and where the image came from: init.upload, else the
// catalog entry init.fresh, else the slot init.slot, else (init.persist)
// the slot booted or saved last, else the default entry of the catalog
async function load(m) {
  // the notebook kernel takes no upload, and only reads the slot
  const notebook = mode === 'notebook', up = notebook ? null : m.upload;
  const v = '?v=' + encodeURIComponent(manifest.build);
  const imageName = manifest.image, changesName = changesOf(imageName);
  const said = { slot: null, slotMissing: undefined, freshMissing: undefined, sourcesMissing: undefined };
  let entry = null;                     // the catalog entry whose image boots
  let image = null, changes = null, source = 'download', savedAt = null, saved = null;
  let webPackage = 0;
  let sources = null;                   // {name, data}: the .sources that came with the image
  if (up) {
    image = await read(up.image);
    changes = await read(up.changes);
    if (up.sources) {
      const name = String(up.sources.name || '');
      if (!/^[^/]+\.sources$/.test(name)) throw new Error('the .sources of the upload is named ' + JSON.stringify(name));
      sources = { name, data: up.sources.data };
    }
    uploadName = typeof up.name === 'string' ? up.name : '';
    source = 'upload';
    webPackage = webPackageOf(image);
  } else {
    // the slot to boot: that of init.slot, or (undefined) the last one
    let slot = null;
    if (typeof m.fresh === 'string' && m.fresh) {
      entry = PharoCatalog.find(manifest, m.fresh);
      if (!entry) said.freshMissing = m.fresh;
    } else if (m.slot !== undefined && m.slot !== null) {
      if (store && typeof m.slot === 'string' && SLOT_ID.test(m.slot)) slot = m.slot;
      else said.slotMissing = String(m.slot);
    } else if (store) {
      slot = undefined;
    }
    if (slot !== null && store) {
      try {
        let report = null;
        saved = await store.load((loaded, total) => (report || (report = progress('restore', total)))(loaded), slot);
        if (!saved && typeof slot === 'string') said.slotMissing = slot;
      } catch (e) {
        storageError = e && e.unavailable
          ? 'this browser does not keep images here (' + errorText(e) + '); use Download to keep a copy'
          : 'the saved image could not be read (' + errorText(e) + ')';
      }
    }
    if (saved) {
      image = saved.image;
      changes = saved.changes;
      sources = saved.sources;
      source = 'saved';
      savedAt = saved.meta.savedAt;
      const kept = PharoStorage.webPackage(saved.meta);
      webPackage = kept === null ? webPackageOf(image) : kept;
      const slotId = saved.meta.slot || MAIN;
      said.slot = { slot: slotId, name: saved.name, pharo: null, imageId: null, origin: saved.meta.origin || null,
                    savedAt };
      if (!notebook) {
        bound = slotId;
        boundName = saved.name;
        owned = saved.meta;
        boundSince = saved.meta.createdAt;
        boundOrigin = saved.meta.origin;
        intended = null;
      }
    } else {
      entry = entry || PharoCatalog.defaultEntry(manifest);
      if (!entry) throw new Error('manifest.json lists no image');
    }
  }
  let wanted, sourcesData = null, served = null, known = null;
  if (entry) {
    // a fresh image of the site: its files, with the version of OSWindow-Web
    // that stage.mjs read from the package that prepared it
    freshEntry = entry;
    wanted = entry.files.slice();
    webPackage = entry.world ? (Number.isInteger(entry.webPackage) && entry.webPackage) || wantedPackage() : 0;
    const s = PharoCatalog.sourcesOf(entry);
    served = known = s ? s.path : null;
    imageId = entry.id;
  } else {
    // the image of a slot or an upload: its own .sources, else the one of
    // the site of the name the slot says or the image has, else (none
    // known) the default one
    let chosen = null;
    if (sources) {
      sourcesData = await read(sources.data);
      chosen = PharoCatalog.sourcesFile(manifest, sources.name);
      // its own unless it is one of the site (which is then not fetched)
      if (!chosen || sourcesData.length !== chosen.size)
        ownSources = { name: sources.name, data: sources.data instanceof Blob ? sources.data : new Blob([sourcesData]) };
      served = known = sources.name;
      wanted = [];
    } else {
      known = (saved && saved.meta.sourcesName) || sourcesNameOf(image);
      chosen = known ? PharoCatalog.sourcesFile(manifest, known)
        : PharoCatalog.sourcesOf(PharoCatalog.defaultEntry(manifest));
      if (known && !chosen) said.sourcesMissing = known;
      served = chosen ? chosen.path : null;
      wanted = chosen ? [chosen] : [];
    }
    // (the entry whose .sources it reads: none when it reads its own)
    const of = ownSources ? null : PharoCatalog.entryOfFile(manifest, chosen);
    imageId = (saved && saved.meta.imageId) || (of ? of.id : null);
  }
  sourcesName = known;
  pharo = (saved && saved.meta.pharo) || PharoCatalog.pharoOf(known) || (entry && entry.pharo) || null;
  if (said.slot) Object.assign(said.slot, { pharo, imageId });
  // a slot of before the versions in its meta: say them, from now on
  if (saved && !notebook && known && (!saved.meta.sourcesName || !saved.meta.pharo)) {
    said.annotate = { sourcesName: known, pharo: PharoCatalog.pharoOf(known), imageId };
  }
  const report = progress('fetch', wanted.reduce((n, f) => n + f.size, 0));
  let loaded = 0;
  const [fetched, st] = await Promise.all([
    Promise.all(wanted.map(f => fetchFile(f, v, n => report(loaded += n)))),
    Promise.all((manifest.st || []).map(p => fetchText(p + v))),
  ]);
  // the files of the slot first, then the placeholders: a file of the
  // manifest of the same name wins
  const files = saved && saved.tree ? treeFiles(saved.tree) : [];
  const home = files.some(f => f.path === HOME && !f.dir) ? null : HOME;
  if (home) files.push({ path: home, dir: true });
  else console.warn('vm-worker: the saved image has a file named ' + HOME + ': the VM keeps the HOME of Emscripten');
  for (const name of manifest.libraries || []) {
    if (!/^[^/]+$/.test(name)) throw new Error('manifest.json names the library file ' + JSON.stringify(name));
    files.push({ path: DIR + '/' + name, data: new Uint8Array(0) });
  }
  wanted.forEach((f, i) => files.push({ path: DIR + '/' + f.path, data: fetched[i] }));
  (manifest.st || []).forEach((p, i) => files.push({ path: DIR + '/' + p, data: st[i] }));
  if (image) {
    files.push({ path: DIR + '/' + imageName, data: image });
    files.push({ path: DIR + '/' + changesName, data: changes });
  }
  if (sourcesData) files.push({ path: DIR + '/' + sources.name, data: sourcesData });
  // An upload becomes a slot once it has booted (keepUpload): a broken one
  // is not kept
  if (up && store) upload = { image, changes };
  // Whether an image is saved in this browser: any slot, one that could not
  // be read too (which resetStorage deletes)
  let stored = false;
  if (store) {
    try { stored = (await store.list()).length > 0; } catch (e) { /* none that can be used */ }
  }
  return Object.assign(said, { files, home, source, savedAt, webPackage, prepared: canOpen(webPackage), stored,
                               sources: served });
}

async function boot(m) {
  // first: resetStorage needs it even when the rest fails
  if (m.persist) {
    try { store = PharoStorage.open(); } catch (e) { storageError = errorText(e); }
  }
  // (and the slot it deletes while this VM has none)
  if (store && mode !== 'notebook' && !m.upload && !(typeof m.fresh === 'string' && m.fresh)) {
    if (m.slot !== undefined && m.slot !== null) intended = typeof m.slot === 'string' && SLOT_ID.test(m.slot) ? m.slot : null;
    else try { intended = await store.last(); } catch (e) { /* none known */ }
  }
  manifest = await fetchManifest(m.manifestUrl || 'manifest.json' + q);
  if (m.build && m.build !== manifest.build)
    console.warn('vm-worker: the page is of build ' + m.build + ', the files of build ' + manifest.build);
  initName = typeof m.name === 'string' ? m.name.trim().slice(0, 200) : '';
  const loaded = await load(m);
  const { files, source, savedAt, webPackage, prepared, stored, sources } = loaded;
  // an image that cannot open the world is prepared for it, in the REPL: one
  // without OSWindow-Web, or with an older version of it
  if (m.prepare && mode === 'world' && manifest.world && !prepared) {
    preparing = 1;
    mode = 'console';
  }
  post({ type: 'progress', phase: 'boot', loaded: 0, total: 1 });
  const config = {};
  if (m.display && !preparing) {
    importScripts('display-worker.js' + q);
    display = PharoDisplay.create(m.display, post);
    config.webDisplay = display;
  }
  const gitProxy = gitProxyOf(m.gitProxy);
  if (gitProxy) config.gitHttpProxy = gitProxy;
  config.gitHttpCredentials = gitCredentialsOf(m.gitProxy);
  config.onGitHttp = gitHttp;
  const image = DIR + '/' + manifest.image;
  const env = mode === 'console' ? { PHARO_WEB_WORLD_FILE: WORLD_FILE } : {};
  if (loaded.home) env.HOME = loaded.home;
  if (m.sliceMs > 0) env.PHARO_WASM_SLICE_MS = String(m.sliceMs);
  const options = {
    args: PharoVMDriver.vmArgs(mode, image),
    events: mode === 'notebook',
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
    onHost: (kind, text) => { if (kind === HOST_IMAGE_SAVED && mode !== 'notebook') imageSaved(text); },
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
  if (bound) {
    // the slot booted is the one to boot next, and says its version
    const annotate = loaded.annotate;
    enqueue(async () => {
      try { await store.setLast(bound); } catch (e) { /* only a convenience */ }
      if (!annotate || !owned) return;
      const meta = await store.annotate(owned, annotate);
      if (meta && owned && owned.id === meta.id) owned = meta;
    });
  }
  const ready = { type: 'ready', image, source, persisted: stored, savedAt, world: !!manifest.world, prepared,
                  webPackage, preparing: !!preparing, sources, storageError, fonts: manifest.fonts || null,
                  git: !!manifest.git, gitHttp: !!(manifest.git && manifest.gitHttp),
                  slot: loaded.slot, imageId, pharo, gitProxy, gitProxyDefault: defaultProxy(),
                  gitCredentials: config.gitHttpCredentials };
  for (const k of ['slotMissing', 'freshMissing', 'sourcesMissing']) if (loaded[k]) ready[k] = loaded[k];
  if (mode === 'notebook') ready.mode = mode;
  post(ready);
  drv.begin();
}

// Module.onGitHttp (gitSupport.c): git's smart-HTTP transport made a
// request, {method, host, proxy, status, error, authorization, retried,
// credentials, proxyRefused}.  Called inside a slice: it only posts.
function gitHttp(r) {
  const status = Number(r && r.status);
  const said = { type: 'git', method: String((r && r.method) || ''), host: String((r && r.host) || ''),
                 proxy: String((r && r.proxy) || ''), status, error: String((r && r.error) || ''),
                 credentials: r && (r.credentials === 'refused' || r.credentials === 'redirected') ? r.credentials : '',
                 redirectedTo: r && r.credentials === 'redirected' ? String(r.redirectedTo || '') : '',
                 proxyRefused: !!(r && r.proxyRefused) };
  if (!gitRequested) {
    gitRequested = true;
    post(Object.assign({ event: 'request' }, said));
  }
  if (r && r.authorization && said.proxy && !gitCredentialed) {
    gitCredentialed = true;
    post(Object.assign({ event: 'credentials' }, said));
  }
  // (a 401 that the same request with credentials follows is no failure)
  if (r && r.retried) return;
  const t = performance.now();
  if ((status < 0 || status >= 400 || said.credentials === 'redirected') && (t - gitFailedAt >= GIT_FAILED_MS || said.credentials)) {
    gitFailedAt = t;
    post(Object.assign({ event: 'failed' }, said));
  }
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
    if (live && mode === 'console') { drv.feed(SAVE + '\n'); stateDirty = true; }
    break;
  case 'saveAs':
    // the next save goes into a new slot of that name
    if (mode === 'notebook') break;
    saveAsName = (typeof m.name === 'string' && m.name.trim().slice(0, 200)) || 'Saved image';
    if (live && mode === 'console') { drv.feed(SAVE + '\n'); stateDirty = true; }
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
      case 'writeFile': {
        const dir = m.path.slice(0, m.path.lastIndexOf('/'));
        if (dir) FS.mkdirTree(dir);
        FS.writeFile(m.path, typeof m.data === 'string' ? m.data : bytes(m.data));
        if (m.mode) FS.chmod(m.path, m.mode & 0o7777);
        if (m.mtime) FS.utime(m.path, m.mtime, m.mtime);
        post({ type: 'fs-result', id: m.id, path: m.path });
        break;
      }
      case 'mkdir':
        FS.mkdirTree(m.path);
        if (m.mode) FS.chmod(m.path, m.mode & 0o7777);
        post({ type: 'fs-result', id: m.id, path: m.path });
        break;
      case 'symlink':
        FS.symlink(String(m.target), m.path);
        post({ type: 'fs-result', id: m.id, path: m.path });
        break;
      case 'stat': {
        const st = FS.lstat(m.path);
        post({ type: 'fs-result', id: m.id, path: m.path, mode: st.mode, size: st.size, mtime: Number(st.mtime),
               link: FS.isLink(st.mode) ? FS.readlink(m.path) : null });
        break;
      }
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
    if (mode === 'notebook') {
      post({ type: 'reset', id: m.id, error: 'not in notebook mode' });
      break;
    }
    // every slot, that one, or the one of this VM: the slot bound, else the
    // one init told it to boot (its boot failed, or the slot could not be
    // read), else none (a fresh image or an upload not kept: nothing of it
    // is stored, and no other image is deleted); a reset of this VM's keeps
    // no upload either.  (Every slot deletes only what is stored: an upload
    // not kept yet is still kept when it booted, and the page may go on.)
    if (!m.all && (m.slot === undefined || m.slot === null)) upload = null;
    enqueue(async () => {
      let sid = null;
      try {
        if (!store) throw new Error(storageError || 'this page does not keep images');
        if (m.all) {
          await store.reset();
        } else {
          sid = m.slot !== undefined && m.slot !== null ? String(m.slot) : bound || intended;
          if (sid) await store.remove(sid);
        }
        if (m.all || (sid && sid === intended)) intended = null;
        if (m.all || (sid && sid === bound)) {
          stopSync();
          bound = owned = boundName = null;
          boundSince = boundOrigin = undefined;
        }
        post({ type: 'reset', id: m.id, slot: sid });
      } catch (e) {
        post({ type: 'reset', id: m.id, slot: sid, error: errorText(e) });
      }
    });
    break;
  case 'flush':
    syncChanges(true);
    break;
  case 'display':
    if (display && live) PharoDisplay.onMessage(m, drv);
    break;
  case 'gitProxy':
    if (drv) {
      drv.module.gitHttpProxy = gitProxyOf(m.gitProxy);
      drv.module.gitHttpCredentials = gitCredentialsOf(m.gitProxy);
    }
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
  mode = m.mode === 'world' || m.mode === 'notebook' ? m.mode : 'console';
  boot(m).catch(e => {
    flushOut();
    post({ type: 'crash', message: 'the VM could not be loaded: ' + errorText(e), stack: String((e && e.stack) || '') });
  }).then(replay);
};
