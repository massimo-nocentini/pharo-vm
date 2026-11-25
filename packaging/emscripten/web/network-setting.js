// network-setting.js - whether Smalltalk code may use the network, as the user chose it
//
// UMD: the global PharoNetwork of the pages (<script>: index.html and
// world.html), a CommonJS module in node.  The choice is the checkbox
// #network-enabled of the Settings dialog (git-proxy.js wires the dialog).
// The pages give it to their VMs: in the worker's init.network, and at once
// when it changes ("network", vm-worker.js).
//
// It is kept in localStorage under KEY: 'off', or absent, which is on, as
// anything else is.  Without storage (a private window, blocked site data)
// it holds for the page, and so does one that could not be kept.  It is set
// only in the dialog, never from the URL.
//
//   PharoNetwork.KEY            'pharo-wasm.network'
//   PharoNetwork.read()         whether the network is on
//   PharoNetwork.write(on)      whether this browser kept it
//   PharoNetwork.onChange(cb)   cb(read()) when another page changed it
//                               (storage events)

(function (root, factory) {
  if (typeof module === 'object' && module.exports) module.exports = factory();
  else root.PharoNetwork = factory();
})(typeof self !== 'undefined' ? self : this, function () {
  'use strict';

  const KEY = 'pharo-wasm.network', OFF = 'off';

  // localStorage, or null (a getter that throws)
  function storage() {
    try { return typeof localStorage !== 'undefined' && localStorage ? localStorage : null; } catch (e) { return null; }
  }

  // The choice of this page, and whether it is newer than the stored one
  let session = true, unkept = false;

  function read() {
    const ls = storage();
    if (!ls || unkept) return session;
    try { return ls.getItem(KEY) !== OFF; } catch (e) { return session; }
  }

  function write(on) {
    session = !!on;
    unkept = true;
    const ls = storage();
    if (ls) {
      try {
        if (session) ls.removeItem(KEY); else ls.setItem(KEY, OFF);
        unkept = false;
      } catch (e) { /* quota, or blocked: it holds for this page */ }
    }
    return !unkept;
  }

  // Another page of the site changed it (a key of null: storage cleared)
  function onChange(cb) {
    if (typeof addEventListener !== 'function') return;
    addEventListener('storage', e => {
      if (e.key !== KEY && e.key !== null) return;
      unkept = false;
      cb(read());
    });
  }

  return { KEY, read, write, onChange };
});
