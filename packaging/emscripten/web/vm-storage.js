// vm-storage.js - keep the images of the pages in the browser (IndexedDB)
//
// UMD: the global PharoStorage in a worker (importScripts) and in the pages
// (<script>: world.html and index.html list the slots), a CommonJS module in
// node (the harnesses).  It uses no DOM.
//
// Slots.  The pages keep any number of images, each in a slot: its .image
// and its .changes, the .sources it reads when that is not one the site
// serves (an image of another Pharo, opened in a page), the files of its
// directory (its tree, below) and its name, as values of the object store
// 'files' of the database of the site, and a record, its meta, that makes
// them a slot.  A slot has an id: 'main', or a sid (newId(), never 'main',
// SLOT_ID matches both).  The keys of a slot are keyOf(slot, file): the
// file itself for main, 'slot/<sid>/<file>' for the others:
//
//   meta                        the record of the slot (below)
//   Pharo.image, Pharo.changes  Blobs
//   Pharo.sources               a Blob, only when the slot has its own
//   Pharo.tree                  {v: 1, entries, data: Blob}, its files
//   Pharo.name                  its name, a string the pages show
//
// main is the slot of before there were several: the keys of the one image
// that the pages kept then are those of main, unchanged (a meta without
// slot is main's), so nothing moves and the database stays at version 1.
// One more key is global: 'last', {slot, at}, the slot that a page booted
// or saved last, which a page boots when it is told none.
//
// The meta: {id, slot, image, changes, imageSize, changesSize, savedAt,
// syncedAt, createdAt, ...}: id is new at every save (newId(): the guards
// below compare it), slot the id of the slot, image and changes the keys of
// its two files, and the rest what the saver added (vm-worker.js: build,
// prepared, webPackage, sourcesName, pharo, imageId, origin), plus, when the
// slot has its own .sources, {sources, sourcesSize}, its key and its size,
// and, when it has a tree, {tree, treeSize, treeFiles}: its key, the bytes
// of its data and its entries.  sourcesName is the name the image reads its
// .sources under, whoever serves it; pharo the version that name says
// ("13.1"), imageId the id of the catalog entry the slot came from (or
// whose .sources it reads), origin how the slot began: 'fresh:<id>',
// 'upload:<file name>' or 'save-as:<slot>' (none for the slots of before).
// A save writes them all in one transaction, so a slot is never half there.
// The name is a key of its own: a page renames a slot without touching its
// meta, and the worker, which stores the meta again at every save and sync,
// never undoes a rename.
//
// The tree.  The files of the directory of the image that are neither the
// image nor the site's (Iceberg's clones in pharo-local, the package cache,
// PharoDebug.log: vm-worker.js says which), as one record: entries
// [[path, kind, mode, mtimeMs, offset, length, linkTarget], ...], path
// relative to the directory, '/'-separated, kind 'f' (a file: its bytes are
// data from offset, length of them), 'd' (a directory, an empty one too)
// or 'l' (a symbolic link to linkTarget), and data, one Blob of the bytes of
// every file.  One record, not one per file: IndexedDB is slow with
// thousands of small values.  checkPath says whether a path of it is one to
// restore (no '', '.' or '..' segment).
//
// webPackage is the version of the OSWindow-Web package that the image of
// the slot has (OSWebDriver class>>packageVersion), as the saver measured it
// from the image: 0 without the package.  The slots saved before there were
// versions have none, only prepared: webPackage(meta) answers what they
// count as.
//
// IndexedDB is per origin, but the sites of an origin are its directories
// (the project sites of GitHub Pages, /stable/ and /preview/ of a host): the
// database of the site in the directory /a/b/ of its origin, where its pages
// and vm-worker.js are, is 'pharo-wasm:/a/b/' (databaseName).  The one at the
// root keeps the name all of them had before, 'pharo-wasm'.
//
//   const store = PharoStorage.open();     // or open(backend)
//   await store.list()                      // [{slot, name, pharo, savedAt, ...}], newest first
//   await store.last()                      // the slot to boot when told none, or null
//   await store.setLast(slot)
//   await store.meta(slot)                  // null, or the meta of the slot (default main)
//   await store.name(slot)                  // null, or the name stored for it
//   await store.load(onProgress, slot)      // null, or {meta, name, image, changes, sources, tree}
//   await store.save(image, changes, extra, sources, kept, target)
//                                           // a slot written; answers its meta
//   await store.syncChanges(changes, meta)  // the .changes of the slot alone
//   await store.annotate(meta, fields)      // fields merged into the meta, same id
//   await store.rename(slot, name)          // its name ('' none)
//   await store.remove(slot)                // the slot no more
//   await store.reset()                     // no slot at all ("Delete all")
//   await store.estimate()                  // {usage, quota}, or null
//   await store.persist(), persisted()      // navigator.storage's, else false
//   PharoStorage.webPackage(meta)           // the version of OSWindow-Web of the slot
//   PharoStorage.keyOf(slot, file), MAIN, SLOT_ID, checkPath(path)
//
// list() answers, for each slot, {slot, name, pharo, imageId, origin,
// createdAt, savedAt, syncedAt, imageSize, changesSize, sourcesName,
// sourcesSize (of its own .sources, else 0), treeSize, treeFiles,
// webPackage, build, prepared}, newest savedAt first; name is the stored
// one, else 'Saved image' for main of before the origins and for a slot
// of no known version, else the label of its version (Pharo 13.1).  It
// needs the keys of the backend (keys()); without them it lists main only.
// last() answers 'last' while that slot is there, else the newest slot.
// load(onProgress, slot) reads the slot (undefined: last()); image and
// changes are Uint8Arrays, sources {name, data (a Uint8Array)} or null, tree
// {entries, data (a Uint8Array)} or null; onProgress(loaded, total) counts
// the bytes of the slot read so far, its tree too.
//
// save(image, changes, extra, sources, kept, target) writes the slot that
// target says: {slot} overwrites that slot as it is (recreating it if it
// was deleted meanwhile), the latest save wins; {slot: null, name} makes a
// new one: main when there is no main (a guarded write that only an absent
// meta passes, so that of two pages saving at once one gets main), else a
// new sid; name is then stored as its name.  Without target: kept's slot,
// or a new one without kept, as before there were several.  image and
// changes are Uint8Arrays.  extra is merged into the meta, except
// extra.tree: {entries, data (a Blob or a Uint8Array), size, files} is
// stored as its tree, null deletes its tree, and undefined keeps the tree
// the slot has (a new slot has none).  sources is the .sources of the slot,
// {name, data} with data a Uint8Array or a Blob, or null when it has none:
// a save without one deletes the one of the slot before.  A .sources is
// large (46 MB for Pharo 15) and never changes, so a save does not write it
// again when kept, the meta of the slot that this page booted from or saved,
// is still the slot written and has a .sources of that name and size: the
// slot then keeps it as stored.  createdAt is kept's when it is that slot,
// else now.
//
// Every method answers a promise, which is rejected when the browser
// refuses (QuotaExceededError, a private window, a blocked database): the
// callers report it, it is never fatal.  When the database cannot be used
// at all, the error has `unavailable' set.
//
// Several pages may run at once (two Console tabs, the Console and the
// world), on one slot or on others.  The image of a slot reads its method
// sources at offsets of its own .changes, so only the page that booted from
// a slot or saved it last may store its .changes again: syncChanges(changes,
// meta) writes only while meta, which load or save answered, is still the
// meta of its slot (the guard is per slot: a save of another slot changes
// nothing), and answers the new meta; once another page saved that slot (or
// removed it) it answers null and writes nothing.  annotate has the same
// guard.
//
// A backend stores values under keys:
//
//   get(key)        the value, or undefined
//   write(entries, guard)
//                   [[key, value], ...] in one transaction; the value
//                   undefined deletes the key.  With guard {key, id} only
//                   when the value under guard.key still has that id, read
//                   in the same transaction, and with {key, id: null} only
//                   when there is no value under guard.key.  Answers whether
//                   it wrote.
//   clear()         deletes every key
//   keys()          every key (optional: without it list() sees main only)
//
// all answering promises.  open() without one takes PharoStorage.backend, or
// the IndexedDB backend when that is null; the worker harness sets it to an
// in-memory backend (PharoStorage.memory) that outlives its workers.

(function (root, factory) {
  if (typeof module === 'object' && module.exports) module.exports = factory();
  else root.PharoStorage = factory();
})(typeof self !== 'undefined' ? self : this, function () {
  'use strict';

  const DATABASE = 'pharo-wasm', VERSION = 1, STORE = 'files';
  const IMAGE = 'Pharo.image', CHANGES = 'Pharo.changes', SOURCES = 'Pharo.sources', META = 'meta';
  const TREE = 'Pharo.tree', NAME = 'Pharo.name', LAST = 'last';
  const MAIN = 'main';
  const SLOT_ID = /^(main|[0-9a-z]{1,12}-[0-9a-z]{1,8}-[0-9a-z]{1,8})$/;
  const SLOT_META = /^slot\/([^/]+)\/meta$/;

  const keyOf = (slot, file) => slot === MAIN ? file : 'slot/' + slot + '/' + file;
  const slotOf = meta => (meta && meta.slot) || MAIN;
  const checkSlot = slot => {
    if (typeof slot !== 'string' || !SLOT_ID.test(slot)) throw new Error('no slot ' + JSON.stringify(slot));
    return slot;
  };
  // Whether a path of a tree is one to restore: relative, with no empty,
  // '.' or '..' segment
  const checkPath = p => typeof p === 'string' && p.length > 0 && p.length < 4096 && !p.includes('\0') &&
    p.split('/').every(s => s !== '' && s !== '.' && s !== '..');
  // (PharoCatalog.labelOf, which this module does not load)
  const labelOf = pharo => {
    const m = /^(\d+)\.(\d+)$/.exec(String(pharo || ''));
    return m ? 'Pharo ' + (Number(m[2]) === 0 ? m[1] : m[1] + '.' + m[2]) : null;
  };

  // The database of the site whose page or worker has the URL href (default
  // the location of this global): DATABASE, and the directory of the site
  // unless it is the root
  function databaseName(href) {
    if (href === undefined) href = typeof location !== 'undefined' ? String(location.href) : '';
    let dir = '/';
    try { dir = new URL('.', href).pathname; } catch (e) { /* no URL: the root */ }
    return dir === '/' ? DATABASE : DATABASE + ':' + dir;
  }

  // The error of a database that cannot be used at all
  function unavailable(e) {
    const u = new Error(String((e && (e.message || e.name)) || e));
    u.name = (e && e.name) || 'Error';
    u.unavailable = true;
    return u;
  }

  // Whether the value under guard.key has the id of the guard, if any, or
  // is absent for the id null
  const guarded = (guard, value) => !guard ||
    (guard.id === null ? value === undefined || value === null : !!value && value.id === guard.id);

  // The backend over IndexedDB (of the worker, or of the page), on the
  // database name (default databaseName()).  The database is opened at the
  // first use.  When another tab upgrades it, this connection closes, and
  // when the browser closes it (the site data was cleared, the storage
  // evicted) it is lost: the next use opens it again, as does a transaction
  // that a connection lost meanwhile refuses.
  function indexedDBBackend(idb, name) {
    let opened = null, current = null;  // the promise of the connection, and it
    // the connection db is gone: the next use opens another one
    const lost = db => { if (current === db) opened = current = null; };
    function database() {
      const factory = idb || (typeof indexedDB !== 'undefined' ? indexedDB : null);
      if (!factory) return Promise.reject(unavailable(new Error('this browser offers no IndexedDB here')));
      if (opened) return opened;
      const p = opened = new Promise((resolve, reject) => {
        const request = factory.open(name || databaseName(), VERSION);
        request.onupgradeneeded = () => {
          if (!request.result.objectStoreNames.contains(STORE)) request.result.createObjectStore(STORE);
        };
        request.onsuccess = () => {
          const db = request.result;
          if (opened === p) current = db;
          db.onversionchange = () => { db.close(); lost(db); };
          db.onclose = () => lost(db);
          resolve(db);
        };
        request.onerror = () => reject(request.error);
      }).catch(e => { if (opened === p) opened = null; throw unavailable(e); });
      return p;
    }
    // f gets the object store and a function to call with the answer; the
    // promise settles with it when the transaction does
    function transaction(mode, f, retried) {
      return database().then(db => {
        let t;
        try { t = db.transaction(STORE, mode); }
        catch (e) {
          // InvalidStateError: the connection closed, which its close event
          // may not have said yet
          if (retried || !e || e.name !== 'InvalidStateError') throw e;
          lost(db);
          return transaction(mode, f, true);
        }
        return new Promise((resolve, reject) => {
          let answer;
          f(t.objectStore(STORE), a => { answer = a; });
          t.oncomplete = () => resolve(answer);
          t.onabort = () => reject(t.error || new Error('the transaction was aborted'));
        });
      });
    }
    function put(s, entries) {
      for (const [key, value] of entries) {
        if (value === undefined) s.delete(key); else s.put(value, key);
      }
    }
    return {
      get: key => transaction('readonly', (s, answer) => {
        const request = s.get(key);
        request.onsuccess = () => answer(request.result);
      }),
      write: (entries, guard) => transaction('readwrite', (s, answer) => {
        if (!guard) { put(s, entries); answer(true); return; }
        const request = s.get(guard.key);
        request.onsuccess = () => {
          const ok = guarded(guard, request.result);
          if (ok) put(s, entries);
          answer(ok);
        };
      }),
      clear: () => transaction('readwrite', s => { s.clear(); }),
      keys: () => transaction('readonly', (s, answer) => {
        const request = s.getAllKeys();
        request.onsuccess = () => answer(request.result);
      }),
    };
  }

  // A backend in a Map, for tests
  function memory(map) {
    map = map || new Map();
    return {
      map,
      get: async key => map.get(key),
      write: async (entries, guard) => {
        if (!guarded(guard, guard && map.get(guard.key))) return false;
        for (const [key, value] of entries) {
          if (value === undefined) map.delete(key); else map.set(key, value);
        }
        return true;
      },
      clear: async () => { map.clear(); },
      keys: async () => [...map.keys()],
    };
  }

  // An id for a new slot or a new save, distinct from those of other pages
  // saving at once
  let saves = 0;
  const newId = now => now.toString(36) + '-' + (++saves).toString(36) + '-' +
    Math.floor(Math.random() * 0x100000000).toString(36);

  // The bytes of a stored value: a Blob (IndexedDB) or what a test stored
  async function bytesOf(value) {
    if (value instanceof Uint8Array) return value;
    if (value && typeof value.arrayBuffer === 'function') return new Uint8Array(await value.arrayBuffer());
    if (value instanceof ArrayBuffer) return new Uint8Array(value);
    throw new Error('the saved image is damaged');
  }
  const blobOf = data => typeof Blob !== 'undefined' && data instanceof Blob ? data : new Blob([data]);
  const sizeOf = data => data.size !== undefined ? data.size : data.length;

  // The fields of a meta that list() answers
  function infoOf(slot, meta, name) {
    const pharo = meta.pharo || null;
    return {
      slot, pharo,
      name: typeof name === 'string' && name ? name
        : slot === MAIN && !meta.origin ? 'Saved image' : labelOf(pharo) || 'Saved image',
      imageId: meta.imageId || null, origin: meta.origin || null, createdAt: meta.createdAt || null,
      savedAt: meta.savedAt || null, syncedAt: meta.syncedAt || null,
      imageSize: meta.imageSize || 0, changesSize: meta.changesSize || 0, sourcesName: meta.sourcesName || null,
      sourcesSize: meta.sources ? meta.sourcesSize || 0 : 0,
      treeSize: meta.tree ? meta.treeSize || 0 : 0, treeFiles: meta.tree ? meta.treeFiles || 0 : 0,
      webPackage: webPackage(meta), build: meta.build || null, prepared: !!meta.prepared,
    };
  }

  function open(backend) {
    const b = backend || api.backend || indexedDBBackend();
    // the slots there are, with their metas: [[slot, meta], ...]
    async function slots() {
      if (typeof b.keys !== 'function') {
        const meta = await b.get(META);
        return meta ? [[MAIN, meta]] : [];
      }
      const ids = [];
      for (const key of await b.keys()) {
        if (key === META) { ids.push(MAIN); continue; }
        const m = SLOT_META.exec(String(key));
        if (m && m[1] !== MAIN && SLOT_ID.test(m[1])) ids.push(m[1]);
      }
      const all = [];
      for (const slot of ids) {
        const meta = await b.get(keyOf(slot, META));
        if (meta) all.push([slot, meta]);
      }
      return all;
    }
    const store = {
      async list() {
        const all = [];
        for (const [slot, meta] of await slots()) all.push(infoOf(slot, meta, await b.get(keyOf(slot, NAME))));
        return all.sort((x, y) => (y.savedAt || 0) - (x.savedAt || 0));
      },
      async last() {
        const last = await b.get(LAST);
        if (last && typeof last.slot === 'string' && SLOT_ID.test(last.slot) && await b.get(keyOf(last.slot, META)))
          return last.slot;
        const all = await store.list();
        return all.length ? all[0].slot : null;
      },
      async setLast(slot) {
        await b.write([[LAST, { slot: checkSlot(slot), at: Date.now() }]]);
      },
      async meta(slot = MAIN) {
        return (await b.get(keyOf(checkSlot(slot), META))) || null;
      },
      async name(slot = MAIN) {
        const name = await b.get(keyOf(checkSlot(slot), NAME));
        return typeof name === 'string' && name ? name : null;
      },
      async load(onProgress, slot) {
        if (slot === undefined) slot = await store.last();
        if (slot === null) return null;
        const metaKey = keyOf(checkSlot(slot), META);
        // the files are read one after the other: when another page saved
        // the slot meanwhile (a new id), read its files instead
        let meta, image, changes, sources, tree, name;
        for (let tries = 0; ; tries++) {
          meta = await b.get(metaKey);
          if (!meta) return null;
          image = await b.get(meta.image);
          changes = await b.get(meta.changes);
          sources = meta.sources ? await b.get(meta.sources) : null;
          tree = meta.tree ? await b.get(meta.tree) : null;
          name = await b.get(keyOf(slot, NAME));
          const now = await b.get(metaKey);
          if (now && now.id === meta.id) break;
          if (tries === 3) throw new Error('the saved image keeps changing');
        }
        if (!image || !changes) return null;
        if (meta.sources && !sources) throw new Error('the .sources of the saved image is missing');
        const treeData = tree && tree.data !== undefined ? tree.data : null;
        const total = (meta.imageSize || 0) + (meta.changesSize || 0) + (meta.sourcesSize || 0) +
          (treeData ? meta.treeSize || 0 : 0);
        let loaded = 0;
        const read = async value => {
          const data = await bytesOf(value);
          loaded += data.length;
          if (onProgress) onProgress(loaded, total);
          return data;
        };
        const imageBytes = await read(image), changesBytes = await read(changes);
        const sourcesBytes = sources ? await read(sources) : null;
        // (a tree that is not there, or not one, is no tree: the image boots)
        const treeBytes = treeData !== null && tree && Array.isArray(tree.entries) ? await read(treeData) : null;
        return { meta, name: infoOf(slot, meta, name).name, image: imageBytes, changes: changesBytes,
                 sources: sourcesBytes ? { name: meta.sourcesName, data: sourcesBytes } : null,
                 tree: treeBytes ? { entries: tree.entries, data: treeBytes } : null };
      },
      async save(image, changes, extra, sources, kept, target) {
        if (target === undefined) target = { slot: kept ? slotOf(kept) : null };
        const now = Date.now();
        const fields = Object.assign({}, extra);
        const tree = fields.tree;
        delete fields.tree;
        // the entries of the slot, and its meta
        const make = async (slot, created) => {
          const k = file => keyOf(slot, file);
          // (kept is of this slot: what it has stays, unless the slot is new)
          const same = !created && !!kept && slotOf(kept) === slot;
          const meta = Object.assign({}, fields, {
            id: newId(now), slot, image: k(IMAGE), changes: k(CHANGES), imageSize: image.length,
            changesSize: changes.length, savedAt: now, syncedAt: now,
            createdAt: same && kept.createdAt ? kept.createdAt : now,
          });
          if (meta.origin === undefined && same && kept.origin) meta.origin = kept.origin;
          const entries = [[k(IMAGE), new Blob([image])], [k(CHANGES), new Blob([changes])], [k(META), meta]];
          let keep = false;               // the .sources stored stays
          if (sources) {
            const size = sizeOf(sources.data);
            Object.assign(meta, { sources: k(SOURCES), sourcesName: sources.name, sourcesSize: size });
            keep = same && !!kept.sources && kept.sourcesName === sources.name && kept.sourcesSize === size;
            if (!keep) entries.push([k(SOURCES), blobOf(sources.data)]);
          } else {
            entries.push([k(SOURCES), undefined]);
          }
          if (tree) {
            entries.push([k(TREE), { v: 1, entries: tree.entries, data: blobOf(tree.data) }]);
            Object.assign(meta, { tree: k(TREE), treeSize: tree.size !== undefined ? tree.size : sizeOf(tree.data),
                                  treeFiles: tree.files !== undefined ? tree.files : tree.entries.length });
          } else if (tree === null || created) {
            entries.push([k(TREE), undefined]);
          } else {
            // the tree of the slot stays, as its meta says
            const was = same ? kept : await b.get(k(META));
            if (was && was.tree === k(TREE))
              Object.assign(meta, { tree: was.tree, treeSize: was.treeSize, treeFiles: was.treeFiles });
            else entries.push([k(TREE), undefined]);
          }
          if (created)
            entries.push([k(NAME), typeof target.name === 'string' && target.name ? target.name : undefined]);
          return { meta, entries, keep };
        };
        const written = async (slot, created, guard) => {
          const { meta, entries, keep } = await make(slot, created);
          // the .sources kept is still stored: only while kept is the slot
          if (keep && await b.write(entries, { key: keyOf(slot, META), id: kept.id })) return meta;
          if (keep) {
            const again = await make(slot, created);
            again.entries.push([keyOf(slot, SOURCES), blobOf(sources.data)]);
            return await b.write(again.entries, guard) ? again.meta : null;
          }
          return await b.write(entries, guard) ? meta : null;
        };
        if (target.slot) return written(checkSlot(target.slot), false, undefined);
        // a new slot: main when there is none, else a sid of its own
        return (await written(MAIN, true, { key: META, id: null })) ||
          written(newId(now), true, undefined);
      },
      async syncChanges(changes, meta) {
        const updated = Object.assign({}, meta, { changesSize: changes.length, syncedAt: Date.now() });
        const key = keyOf(slotOf(meta), META);
        const wrote = await b.write([[meta.changes, new Blob([changes])], [key, updated]], { key, id: meta.id });
        return wrote ? updated : null;
      },
      async annotate(meta, fields) {
        const updated = Object.assign({}, meta, fields, { id: meta.id, slot: meta.slot, image: meta.image,
                                                          changes: meta.changes });
        if (meta.slot === undefined) delete updated.slot;
        const key = keyOf(slotOf(meta), META);
        return await b.write([[key, updated]], { key, id: meta.id }) ? updated : null;
      },
      async rename(slot, name) {
        await b.write([[keyOf(checkSlot(slot), NAME), typeof name === 'string' && name ? name : undefined]]);
      },
      async remove(slot) {
        const k = file => keyOf(checkSlot(slot), file);
        const entries = [META, IMAGE, CHANGES, SOURCES, TREE, NAME].map(f => [k(f), undefined]);
        const last = await b.get(LAST);
        if (last && last.slot === slot) entries.push([LAST, undefined]);
        await b.write(entries);
      },
      reset() { return b.clear(); },
      async estimate() {
        const storage = typeof navigator !== 'undefined' && navigator.storage;
        if (!storage || !storage.estimate) return null;
        try { return await storage.estimate(); } catch (e) { return null; }
      },
      async persist() {
        const storage = typeof navigator !== 'undefined' && navigator.storage;
        if (!storage || !storage.persist) return false;
        try { return !!(await storage.persist()); } catch (e) { return false; }
      },
      async persisted() {
        const storage = typeof navigator !== 'undefined' && navigator.storage;
        if (!storage || !storage.persisted) return false;
        try { return !!(await storage.persisted()); } catch (e) { return false; }
      },
    };
    return store;
  }

  // The version of OSWindow-Web of the image of the slot with this meta: its
  // webPackage, else, for a slot saved before there were versions, 1 when its
  // image could open the world (it had the package, which then had none) and
  // 0 when it could not; null when the meta does not say (the caller looks
  // at the image)
  function webPackage(meta) {
    if (!meta) return null;
    if (Number.isInteger(meta.webPackage) && meta.webPackage >= 0) return meta.webPackage;
    if (meta.prepared === undefined) return null;
    return meta.prepared ? 1 : 0;
  }

  const api = { open, memory, indexedDB: indexedDBBackend, databaseName, webPackage, keyOf, checkPath, newId,
                backend: null, DATABASE, STORE, MAIN, SLOT_ID };
  return api;
});
