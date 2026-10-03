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
// PHARO_WASM_STATS=1 prints the slice counters (_vm_stats) at the end.

Module['onVMStarted'] = () => {
  const BUSY = 2, SLEEPING = 4;
  const stats = !!process.env.PHARO_WASM_STATS && process.env.PHARO_WASM_STATS != '0';
  const pump = () => {
    let state;
    try {
      state = Module['_vm_resume']();
    } catch (e) {
      if (stats) Module['_vm_stats']();
      if (e && e.name == 'ExitStatus') return;
      // A trap or abort(): report it without node's echo of the minified line.
      process.stderr.write(`${(e && e.stack) || e}\n`);
      process.exitCode = 1;
      return;
    }
    if (state == BUSY) setImmediate(pump);
    else if (state == SLEEPING) setTimeout(pump, Math.max(1, Module['_vm_wakeup_ms']()));
    else if (stats) Module['_vm_stats']();
  };
  // Called from inside main(): run the next slice from the event loop.
  setImmediate(pump);
};
