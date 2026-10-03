// node-pre.js - --pre-js for the node CLI (-sNODERAWFS) of the Pharo VM on wasm
//
// NODERAWFS (emscripten 6.0.10) opens a path with node's fs.openSync, which
// follows symbolic links, but gives the stream a node whose mode comes from
// lstat.  For a symbolic link to a directory, opendir() then succeeds while
// the first readdir() fails with ENOTDIR (getdents64 looks entries up
// under a "parent" that is not a directory), so the image sees such a
// directory as empty.  Give the stream the mode of what was actually
// opened.  Runs after NODERAWFS installed itself into FS.

Module['preRun'] = [].concat(Module['preRun'] || [], () => {
  if (typeof FS != 'object' || typeof FS.open != 'function') return;
  const fs = require('fs');
  const open = FS.open;
  FS.open = (...args) => {
    const stream = open(...args);
    if (stream && stream.node && typeof stream.nfd == 'number' && FS.isLink(stream.node.mode))
      stream.node.mode = fs.fstatSync(stream.nfd).mode;
    return stream;
  };
});
