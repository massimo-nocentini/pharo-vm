// vm-storage.js - keep the image of the pages in the browser (IndexedDB)
//
// UMD: the global PharoStorage in a worker (importScripts), a CommonJS module
// in node (the worker harness).  It uses no DOM.
//
// The pages keep one image, the one saved last: its .image and its .changes,
// as Blobs in the object store 'files' of the database of the site, under the
// keys 'Pharo.image' and 'Pharo.changes', and a record 'meta' that makes them
// a slot: {id, image, changes, imageSize, changesSize, savedAt, syncedAt, ...}
// with an id of its own, the names of the two files and what the saver added
// (the build, ...).  A save writes all three in one transaction, so a slot is
// never half there.
//
// IndexedDB is per origin, but the sites of an origin are its directories
// (the project sites of GitHub Pages, /stable/ and /preview/ of a host): the
// database of the site in the directory /a/b/ of its origin, where its pages
// and vm-worker.js are, is 'pharo-wasm:/a/b/' (databaseName).  The one at the
// root keeps the name all of them had before, 'pharo-wasm'.
//
//   const store = PharoStorage.open();     // or open(backend)
//   await store.load(onProgress)            // null, or {meta, image, changes}
//   await store.meta()                      // null, or the meta of the slot
//   await store.save(image, changes, extra) // a new slot; answers its meta
//   await store.syncChanges(changes, meta)  // the .changes of the slot alone
//   await store.reset()                     // no slot any more
//   await store.estimate()                  // {usage, quota}, or null
//
// image and changes are Uint8Arrays.  onProgress(loaded, total) counts the
// bytes of the slot read so far.  Every method answers a promise, which is
// rejected when the browser refuses (QuotaExceededError, a private window, a
// blocked database): the callers report it, it is never fatal.  When the
// database cannot be used at all, the error has `unavailable' set.
//
// Several pages may run at once (two Console tabs, the Console and the
// world) and share the slot.  The image of a slot reads its method sources
// at offsets of its own .changes, so only the page that booted from a slot
// or saved it may store its .changes again: syncChanges(changes, meta)
// writes only while meta, which load or save answered, is still the slot,
// and answers the new meta; once another page saved (or reset) it answers
// null and writes nothing.
//
// A backend stores values under keys:
//
//   get(key)        the value, or undefined
//   write(entries, guard)
//                   [[key, value], ...] in one transaction; the value
//                   undefined deletes the key.  With guard {key, id} only
//                   when the value under guard.key still has that id, read
//                   in the same transaction.  Answers whether it wrote.
//   clear()         deletes every key
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
  const IMAGE = 'Pharo.image', CHANGES = 'Pharo.changes', META = 'meta';

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

  // Whether the value under guard.key has the id of the guard, if any
  const guarded = (guard, value) => !guard || (!!value && value.id === guard.id);

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
    };
  }

  // An id for a new slot, distinct from those of other pages saving at once
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

  function open(backend) {
    const b = backend || api.backend || indexedDBBackend();
    return {
      async load(onProgress) {
        // the three are read one after the other: when another page saved
        // meanwhile (a new id), read its slot instead
        let meta, image, changes;
        for (let tries = 0; ; tries++) {
          meta = await b.get(META);
          if (!meta) return null;
          image = await b.get(meta.image);
          changes = await b.get(meta.changes);
          const now = await b.get(META);
          if (now && now.id === meta.id) break;
          if (tries === 3) throw new Error('the saved image keeps changing');
        }
        if (!image || !changes) return null;
        const total = (meta.imageSize || 0) + (meta.changesSize || 0);
        const imageBytes = await bytesOf(image);
        if (onProgress) onProgress(imageBytes.length, total);
        const changesBytes = await bytesOf(changes);
        if (onProgress) onProgress(imageBytes.length + changesBytes.length, total);
        return { meta, image: imageBytes, changes: changesBytes };
      },
      // whether there is a slot, without reading its files
      async meta() {
        return (await b.get(META)) || null;
      },
      async save(image, changes, extra) {
        const now = Date.now();
        const meta = Object.assign({}, extra, {
          id: newId(now), image: IMAGE, changes: CHANGES, imageSize: image.length, changesSize: changes.length,
          savedAt: now, syncedAt: now,
        });
        await b.write([[IMAGE, new Blob([image])], [CHANGES, new Blob([changes])], [META, meta]]);
        return meta;
      },
      async syncChanges(changes, meta) {
        const updated = Object.assign({}, meta, { changesSize: changes.length, syncedAt: Date.now() });
        const wrote = await b.write([[meta.changes, new Blob([changes])], [META, updated]], { key: META, id: meta.id });
        return wrote ? updated : null;
      },
      reset() { return b.clear(); },
      async estimate() {
        const storage = typeof navigator !== 'undefined' && navigator.storage;
        if (!storage || !storage.estimate) return null;
        try { return await storage.estimate(); } catch (e) { return null; }
      },
    };
  }

  const api = { open, memory, indexedDB: indexedDBBackend, databaseName, backend: null, DATABASE, STORE };
  return api;
});
