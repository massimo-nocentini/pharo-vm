// vm-driver.js - drive the WebAssembly Pharo VM (pharo-web.js) from JavaScript
//
// UMD: a CommonJS module in node (the harnesses), the global PharoVMDriver in
// a worker (importScripts).  It uses no DOM and no worker API.
//
//   const vm = await PharoVMDriver.start(createPharoVM, options);
//   ...                                  // e.g. post 'ready' to the page
//   vm.begin();
//
// The VM runs in slices (src/emscripten/emscriptenMain.c).  main() only gets
// it ready, and every slice is a call of _vm_resume(), which answers how the
// slice ended.  The driver runs them: soon (schedule) when the slice was
// merely over (BUSY), after _vm_wakeup_ms() (later) when the image is idle
// (SLEEPING).  Input, Stop and kick() cut such a sleep short.  Exports are
// called only between slices, and the driver keeps no view of the memory,
// which a memory growth detaches: exports take and answer numbers only.
//
// start() writes the files into MEMFS, installs the stdio devices and
// creates the module, whose main() runs at once.  It answers the driver, or
// null when the VM did not start (onCrash has then been called), and
// rejects only when the module fails to load or instantiate, calling no
// callback.  begin() runs the first slice, which loads the image.  Options:
//
//   args         the VM's arguments (default vmArgs('console'))
//   files        [{path, data}] written into MEMFS before main() runs, with
//                the directories they need; data is a Uint8Array or a string
//   cwd          the working directory (default '/pharo', created)
//   thisProgram  argv[0] (default '/pharo/pharo': the VM's directory is /pharo)
//   env          {name: value} added to the environment of the VM
//   sliceMs      the time slice in ms (default PHARO_WASM_SLICE_MS of the build)
//   config       more properties of the emscripten Module (M2: webDisplay;
//                sdl.html: canvas)
//   wasmModule   a precompiled WebAssembly.Module (optional)
//   locateFile   emscripten's locateFile hook (optional)
//   schedule(f)  run f soon, as a macrotask, so that JS events get a turn
//                (default setTimeout(f, 0), which browsers clamp to 4 ms)
//   later(f, ms) run f after ms milliseconds, answering a handle
//   cancel(h)    forget a later() (default clearTimeout)
//   canRun()     optional backpressure: false stops running BUSY slices until
//                resumeOutput() is called
//   onOutput(fd, bytes)  what the VM writes to fd 1 or 2, every write as it
//                comes, as a Uint8Array of its own; called inside the slice
//   onState(st)  after every slice: BUSY, SLEEPING or WAITING
//   onHost(kind, text)   a notification of the VM, after the slice that made
//                it: HOST_IMAGE_SAVED with the path of the image
//   onExit(code), onCrash(message, stack, stacks)  called once; the driver
//                is then dead.  stacks is what _vm_dump_stacks() printed
//   onDiag(text) messages of the emscripten runtime itself (default
//                console.warn)
//
// What a callback (or canRun()) throws is reported with onDiag, as
// "vm-driver: onState: <the error>", and the VM goes on: it must neither
// unwind a slice nor leave the VM without its next one.
//
// stdin (fd 0) is the terminal, which feed() and eof() fill.  A read answers
// what was fed, fails with EAGAIN while nothing is there and answers 0 after
// eof(); a poll answers POLLIN only when there is input or its end.  So the
// image waits for input as at a console (web-repl.st: Stdio stdin
// waitForData) and the VM sleeps meanwhile; a SLEEPING slice is reported as
// WAITING when stdin had nothing to give since the last input.  Every write
// to fd 1 and fd 2 reaches onOutput at once, without line buffering, so a
// prompt shows.
//
// The VM exits with exit(), whose ExitStatus unwinds out of _vm_resume()
// (emscripten 6.0.10 has no Module.quit to override): onExit(status), also
// during the first slice, e.g. when the image cannot be loaded.  Any other
// exception out of a slice, a trap or an abort(), is a crash.

(function (root, factory) {
  if (typeof module === 'object' && module.exports) module.exports = factory();
  else root.PharoVMDriver = factory();
})(typeof self !== 'undefined' ? self : this, function () {
  'use strict';

  // The states of vm_resume() (include/pharovm/emscripten/sqPlatformSpecific.h);
  // WAITING is the driver's own.
  const RUNNING = 0, WAITING = 1, BUSY = 2, EXITED = 3, SLEEPING = 4;
  // The kinds of Module.onPharoHost (src/emscripten/emscriptenSupport.c)
  const HOST_IMAGE_SAVED = 1;
  const POLLIN = 1, POLLRDNORM = 64, EAGAIN = 6;
  // the longest the driver leaves a sleeping VM alone
  const MAX_SLEEP_MS = 1000;

  const encoder = new TextEncoder();

  // The arguments of the VM for the modes of the pages: 'console', the REPL
  // of web-repl.st (filed in at every boot, so without logging its source
  // to the .changes again each time), 'world', the Morphic world, and 'sdl',
  // the same world, which the image then opens through its OSSDL2Driver
  // (sdl.html), the VM having no display for its OSWebDriver.
  function vmArgs(mode, image) {
    image = image || '/pharo/Pharo.image';
    if (mode === 'world' || mode === 'sdl') return ['--headless', image, '--no-default-preferences', '--interactive'];
    return ['--headless', image, '--no-default-preferences', 'st', '--no-source', '/pharo/st/web-repl.st'];
  }

  async function start(createModule, o) {
    const schedule = o.schedule || (f => setTimeout(f, 0));
    const later = o.later || ((f, ms) => setTimeout(f, ms));
    const cancel = o.cancel || (h => clearTimeout(h));
    const diag = o.onDiag || (t => console.warn(t));
    let M = null, FS = null, dead = false, inSlice = false, last = RUNNING;
    let next = null;                    // the coming slice: true (schedule) or {timer}
    const notes = [];                   // the onPharoHost notifications of the slice
    let capture = null;                 // collects what _vm_dump_stacks() prints
    const input = [];                   // the chunks fed and not read yet
    let inputOffset = 0, inputEnd = false, inputRefused = false, stdinNode = null;

    function readStdin(stream, buffer, offset, length) {
      if (!input.length) {
        if (inputEnd) return 0;
        inputRefused = true;
        throw new FS.ErrnoError(EAGAIN);
      }
      const into = new Uint8Array(buffer.buffer, buffer.byteOffset + offset, length);
      let n = 0;
      while (n < length && input.length) {
        const chunk = input[0], k = Math.min(length - n, chunk.length - inputOffset);
        into.set(chunk.subarray(inputOffset, inputOffset + k), n);
        n += k;
        inputOffset += k;
        if (inputOffset === chunk.length) { input.shift(); inputOffset = 0; }
      }
      return n;
    }
    function pollStdin() {
      if (input.length || inputEnd) return POLLIN | POLLRDNORM;
      inputRefused = true;
      return 0;
    }
    // aio waits on stdin through epoll, which listens to the node
    function inputArrived() {
      inputRefused = false;
      if (stdinNode && stdinNode.notifyListeners) stdinNode.notifyListeners(POLLIN);
      runSoon();
    }
    // Call the callback name of the host with args, if it has one; what it
    // throws goes to onDiag.  Answers what it answered, or fallback.
    function host(name, args, fallback) {
      const f = o[name];
      if (!f) return fallback;
      try { return f.apply(o, args); }
      catch (e) { diag('vm-driver: ' + name + ': ' + ((e && e.stack) || e) + '\n'); return fallback; }
    }
    // The buffer is HEAP8, or a copy of the iovecs of a writev
    function writer(fd) {
      return (stream, buffer, offset, length) => {
        const bytes = new Uint8Array(buffer.buffer, buffer.byteOffset + offset, length).slice();
        if (capture) capture.push(bytes);
        else if (length) host('onOutput', [fd, bytes]);   // inside the slice
        return length;
      };
    }

    function runSoon() {
      if (dead || next === true) return;
      if (next) cancel(next.timer);
      next = true;
      schedule(pump);
    }
    function runLater(ms) {
      if (dead || next) return;
      const job = {};
      job.timer = later(() => { if (next === job) pump(); }, ms);
      next = job;
    }
    function pump() {
      next = null;
      if (dead) return;
      let st, failure = null;
      inSlice = true;
      try { st = M._vm_resume(); } catch (e) { failure = e; }
      inSlice = false;
      // the notifications of the slice come before how it ended: a save
      // before the exit that followed it
      while (notes.length) host('onHost', notes.shift());
      if (failure) fail(failure); else report(st);
    }
    function report(st) {
      if (dead) return;
      if (st !== BUSY && st !== SLEEPING) {
        fail(new Error('the interpreter returned (_vm_resume() answered ' + st + ')'));
        return;
      }
      if (st === SLEEPING && inputRefused) st = WAITING;
      last = st;
      host('onState', [st]);
      if (dead) return;                 // onState ended the VM: an interrupt() that failed
      if (st === BUSY) { if (host('canRun', [], true)) runSoon(); }   // else resumeOutput()
      else runLater(Math.min(MAX_SLEEP_MS, Math.max(1, M._vm_wakeup_ms())));
    }
    // The VM is dead; then call the host's callback name with args
    function finish(name, args) {
      if (dead) return;
      dead = true;
      last = EXITED;
      if (next && next !== true) cancel(next.timer);
      next = null;
      host(name, args);
    }
    function fail(e) {
      if (e && e.name === 'ExitStatus') { finish('onExit', [e.status]); return; }
      // the Smalltalk stacks, if the VM can still print them
      capture = [];
      try { M._vm_dump_stacks(); } catch (_) { /* it cannot */ }
      const stacks = new TextDecoder().decode(concat(capture));
      capture = null;
      const message = String((e && e.message) || e), stack = String((e && e.stack) || '');
      finish('onCrash', [message, stack, stacks]);
    }

    const opts = Object.assign({}, o.config, {
      arguments: o.args || vmArgs('console'),
      thisProgram: o.thisProgram || '/pharo/pharo',
      print: t => diag(t + '\n'),
      printErr: t => diag(t + '\n'),
      // called inside a slice: only queue it
      onPharoHost: (kind, text) => { notes.push([kind, text]); },
      preRun: [mod => {
        FS = mod.FS;
        const cwd = o.cwd || '/pharo';
        FS.mkdirTree(cwd);
        FS.chdir(cwd);
        for (const f of o.files || []) {
          const dir = f.path.slice(0, f.path.lastIndexOf('/'));
          if (dir) FS.mkdirTree(dir);
          FS.writeFile(f.path, f.data);
        }
        for (const name in o.env || {}) mod.ENV[name] = String(o.env[name]);
        // stdin stays the terminal (/dev/tty), as for a user at a console:
        // the FilePlugin reads a terminal with a non-blocking read(), and
        // anything else through fread(), whose buffer aio cannot see.  Its
        // stream gets operations of its own, the terminal's being shared.
        // stdout and stderr get devices of their own, whose operations
        // (shared by every stream of the device) are then replaced.
        FS.init(undefined, () => {}, () => {});
        const stdin = FS.getStream(0);
        stdinNode = stdin.node;
        stdin.stream_ops = Object.assign({}, stdin.stream_ops, { read: readStdin, poll: pollStdin });
        Object.assign(FS.getStream(1).stream_ops, { write: writer(1) });
        Object.assign(FS.getStream(2).stream_ops, { write: writer(2) });
      }],
    });
    if (o.locateFile) opts.locateFile = o.locateFile;
    let loadFailed;
    const loadFailure = new Promise((_, reject) => { loadFailed = reject; });
    if (o.wasmModule) opts.instantiateWasm = (imports, ok) => {
      WebAssembly.instantiate(o.wasmModule, imports)
        .then(instance => ok(instance, o.wasmModule), loadFailed);
      return {};
    };

    M = await Promise.race([createModule(opts), loadFailure]);
    if (!M._vm_started()) {
      finish('onCrash', ['the VM did not start', '', '']);
      return null;
    }
    if (o.sliceMs > 0) M._vm_set_slice_ms(o.sliceMs);

    return {
      // after the host is ready for onState: run the first slice
      begin() { runSoon(); },
      // input on stdin: a string goes as UTF-8; ignored after eof()
      feed(data) {
        if (dead || inputEnd) return;
        const bytes = typeof data === 'string' ? encoder.encode(data) : new Uint8Array(data);
        if (!bytes.length) return;
        input.push(bytes);
        inputArrived();
      },
      // the end of the input: reads answer 0 once the input is read
      eof() {
        if (dead || inputEnd) return;
        inputEnd = true;
        inputArrived();
      },
      // Stop: signal the semaphore the image registered with the WebHostPlugin
      // (WebRepl, OSWebDriver).  Answers whether there was one.  Not from
      // onOutput, which runs inside a slice.
      interrupt() {
        if (dead) return false;
        if (inSlice) throw new Error('vm-driver: interrupt() inside a slice');
        let registered = false;
        try { registered = M._vm_interrupt() === 1; } catch (e) { fail(e); return false; }
        runSoon();
        return registered;
      },
      // run a slice soon, e.g. after an event given to the VM through an export
      kick() { runSoon(); },
      // canRun() may answer true again
      resumeOutput() { if (!dead && last === BUSY) runSoon(); },
      // what the last slice reported: RUNNING before the first, EXITED once dead
      state() { return dead ? EXITED : last; },
      get FS() { return M.FS; },
      get module() { return M; },
      get dead() { return dead; },
    };
  }

  function concat(chunks) {
    const all = new Uint8Array(chunks.reduce((n, c) => n + c.length, 0));
    let at = 0;
    for (const c of chunks) { all.set(c, at); at += c.length; }
    return all;
  }

  return { start, vmArgs, RUNNING, WAITING, BUSY, EXITED, SLEEPING, HOST_IMAGE_SAVED };
});
