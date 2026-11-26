// node-pump.js - --post-js for the node CLI (-sNODERAWFS) of the Pharo VM on wasm
//
// The VM runs in slices (src/emscripten/emscriptenMain.c).  main() calls
// Module.onVMStarted and returns to node with the runtime kept alive; every
// slice is then a call to _vm_resume(), which answers what to do next.  Pump
// it from node's event loop: at once when the slice was merely over (BUSY),
// after the wakeup time when the image is idle (SLEEPING).  When the VM exits,
// exit() unwinds out of _vm_resume() as an ExitStatus, whose status
// EXIT_RUNTIME=1 already made the exit code.
//
// A host event that signals the image (host-events.js) wakes it: hostWake()
// runs the next slice at once.  When the VM is gone, however it ended,
// pharoHost.shutdown() drops what would keep node alive.  All but a
// WebSocket of node whose peer does not answer its close: it keeps the
// event loop alive until the peer ends TCP.  When the WebSocket service
// (web-socket.js) has such lingering sockets after the shutdown, an
// unref()'d timer exits 1500 ms later with the status of the VM, once
// stdout and stderr took what was written; a run without one arms nothing.
//
// PHARO_WASM_STATS=1 prints the slice counters (_vm_stats) at the end, and
// whether that exit guard was armed (exitGuard=1, else 0).
// PHARO_WASM_NETWORK=0 turns the network of the image off (fetch, the
// WebSockets of the image and git): a convenience and no boundary, the
// sockets, the FFI and the files of the host being there.

if (process.env.PHARO_WASM_NETWORK === '0') Module['hostNetwork'] = false;

Module['onVMStarted'] = () => {
  const BUSY = 2, SLEEPING = 4;
  const stats = !!process.env.PHARO_WASM_STATS && process.env.PHARO_WASM_STATS != '0';
  let timer = null, immediate = null, gone = false;     // the next slice: one of them
  const soon = () => {
    if (gone || immediate) return;
    clearTimeout(timer);
    timer = null;
    immediate = setImmediate(pump);
  };
  const end = () => {
    gone = true;
    // the slice that ended the VM may have woken it
    clearImmediate(immediate);
    clearTimeout(timer);
    Module['pharoHost'].shutdown();
    const guard = !!Module['webSocket'] && Module['webSocket'].lingering() > 0;
    if (guard) {
      const exit = setTimeout(() => {
        process.stdout.write('', () => process.stderr.write('', () => process.exit(process.exitCode)));
      }, 1500);
      exit.unref();
    }
    if (stats) {
      Module['_vm_stats']();
      process.stderr.write(`[pharo-wasm] exitGuard=${guard ? 1 : 0}\n`);
    }
  };
  const pump = () => {
    immediate = timer = null;
    let state;
    try {
      state = Module['_vm_resume']();
    } catch (e) {
      end();
      if (e && e.name == 'ExitStatus') return;
      // A trap or abort(): report it without node's echo of the minified line.
      process.stderr.write(`${(e && e.stack) || e}\n`);
      process.exitCode = 1;
      return;
    }
    if (state == BUSY) soon();
    else if (state == SLEEPING) {
      if (!immediate) timer = setTimeout(pump, Math.max(1, Module['_vm_wakeup_ms']()));
    } else end();
  };
  Module['hostWake'] = soon;
  // Called from inside main(): run the next slice from the event loop.
  soon();
};
