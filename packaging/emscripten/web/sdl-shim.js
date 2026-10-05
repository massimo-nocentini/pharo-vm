// sdl-shim.js - the DOM that SDL2's Emscripten video driver needs, in the VM's worker
//
// vm-worker.js loads it with importScripts in its 'sdl' mode (sdl.html,
// built with WASM_SDL2=ON), where the image draws its world through its own
// OSSDL2Driver: uFFI calls SDL2 2.32.10, linked into the VM
// (cmake/emscripten/deps/sdl2.cmake), whose Emscripten video driver draws
// into the OffscreenCanvas that the page transferred.  That driver, and
// Emscripten's html5 library under it, expect the DOM of the main thread,
// which a worker lacks.  This shim gives them the few objects they touch:
//
//   1. screen                  emscripten_get_screen_size (SDL's display mode)
//   2. devicePixelRatio        1: a canvas pixel per CSS pixel
//   3. canvas.getBoundingClientRect, which emscripten_get_element_css_size
//                              and the mouse events read: the canvas at 0,0,
//                              of its own size (the page sends coordinates
//                              relative to the canvas, one per pixel)
//   4. canvas.style            SDL_SetCursor and SDL_ShowCursor write its
//                              cursor, which goes to the page
//   5. canvas.onwheel          null: html5 registers its wheel callback only
//                              on a target that has the property
//   6. document                an EventTarget, where SDL listens to mouseup;
//                              querySelector('#canvas') answers the canvas;
//                              body and documentElement with a style;
//                              createElement('canvas') for SDL_CreateCursor;
//                              and a title, which SDL_SetWindowTitle writes
//                              and which goes to the page
//   7. navigator.userActivation  inactive: html5 reads isActive after every
//                              event it handled, to run deferred requests
//                              (fullscreen, pointer lock), which a worker
//                              never may
//
// Items 1, 2, 6 and 7 are globals of the worker, so install() must run
// before the factory of the VM (createPharoVM): html5's specialHTMLTargets,
// [0, document, window], is made when the factory runs.  The worker also
// makes the canvas Module.canvas (the config of PharoVMDriver.start), and
// attach(vm) makes it specialHTMLTargets['#canvas'] when the build exports
// specialHTMLTargets; else '#canvas' is found through document.querySelector.
// window is not defined: Emscripten would take the worker for the main
// thread.  So what SDL registers on the window (focus, blur, resize) finds
// no target, and is not bridged.  The VM sets SDL_EMSCRIPTEN_KEYBOARD_ELEMENT
// to '#canvas' (vm-worker.js), so that the keys go to the canvas too.
//
//   const shim = PharoSDLShim.install(canvas, post);
//   ... PharoVMDriver.start(createPharoVM, {config: {canvas}, ...})
//   shim.attach(vm);                     // once the VM started
//   shim.dispatch(record);               // a DOM event of the page, between slices
//   shim.stats()                         // what the VM and the presents cost
//
// The page forwards its DOM events as records {type, clientX, clientY,
// button, key, ...}: the fields of the event that html5 reads, with clientX
// and clientY relative to the canvas.  dispatch() makes an Event of each one
// and dispatches it on the canvas, or on the document for mouseup, where
// SDL's handlers (html5's, which call into the VM) only queue SDL events.
// So dispatch() must be called between slices, never inside one.
//
// post(message) sends the page what SDL asks for, {kind, ...}:
//
//   title {title}              SDL_SetWindowTitle
//   cursor {cursor}            a CSS cursor: a name, 'none', or
//                              'url(pharo-sdl-cursor:<n>) <x> <y>, auto' for
//                              cursor image n
//   cursorImage {id, width, height, rgba}
//                              the pixels (RGBA, an ArrayBuffer) of cursor
//                              image n, posted before any cursor that names
//                              it: SDL_CreateCursor makes a data URL of a
//                              canvas, synchronously, which an OffscreenCanvas
//                              cannot, so the page makes it
//   size {width, height}       the canvas has that size now (SDL_CreateWindow,
//                              SDL_SetWindowSize)
//   painted {ms}               the first present was drawn, ms after install()
//
// stats() answers {slices, sliceMs, presents, presentMs, pixels,
// firstPresentMs}: the slices of the VM and the time spent in them, the
// putImageData calls of SDL's framebuffer on the canvas (each one the whole
// window: SDL ignores the dirty rectangles), their time and pixels, and when
// the first one came, in ms after install().  They count from install().

(function (root, factory) {
  if (typeof module === 'object' && module.exports) module.exports = factory();
  else root.PharoSDLShim = factory();
})(typeof self !== 'undefined' ? self : this, function () {
  'use strict';

  const CURSOR_SCHEME = 'pharo-sdl-cursor:';

  function install(canvas, post) {
    const t0 = performance.now();
    const stats = { slices: 0, sliceMs: 0, presents: 0, presentMs: 0, pixels: 0, firstPresentMs: null };
    const style = text => String(text).slice(0, 4096);

    // 1, 2
    const size = { width: canvas.width, height: canvas.height };
    globalThis.screen = { width: size.width, height: size.height, availWidth: size.width, availHeight: size.height,
                          colorDepth: 24, pixelDepth: 24 };
    globalThis.devicePixelRatio = 1;

    // 3, 4, 5 on the canvas itself, and its size, which goes to the page
    const proto = Object.getPrototypeOf(canvas);
    for (const name of ['width', 'height']) {
      const d = Object.getOwnPropertyDescriptor(proto, name) ||
        Object.getOwnPropertyDescriptor(OffscreenCanvas.prototype, name);
      Object.defineProperty(canvas, name, {
        configurable: true,
        get() { return d.get.call(this); },
        set(v) {
          d.set.call(this, v);
          const w = this.width, h = this.height;
          if (w !== size.width || h !== size.height) {
            size.width = w;
            size.height = h;
            post({ kind: 'size', width: w, height: h });
          }
        },
      });
    }
    canvas.getBoundingClientRect = function () {
      const w = this.width, h = this.height;
      return { left: 0, top: 0, x: 0, y: 0, width: w, height: h, right: w, bottom: h };
    };
    canvas.onwheel = null;
    const canvasStyle = {};
    let cursor = '';
    Object.defineProperty(canvasStyle, 'cursor', {
      get() { return cursor; },
      set(v) {
        v = style(v);
        if (v === cursor) return;
        cursor = v;
        post({ kind: 'cursor', cursor: v });
      },
    });
    canvas.style = canvasStyle;

    // the canvases of SDL_CreateCursor: their toDataURL names the pixels,
    // which go to the page
    let cursors = 0;
    function cursorCanvas() {
      const c = new OffscreenCanvas(1, 1);
      c.toDataURL = function () {
        const id = ++cursors, w = this.width, h = this.height;
        const ctx = this.getContext('2d');
        const rgba = w && h ? ctx.getImageData(0, 0, w, h).data.buffer : new ArrayBuffer(0);
        post({ kind: 'cursorImage', id, width: w, height: h, rgba }, [rgba]);
        return CURSOR_SCHEME + id;
      };
      return c;
    }

    // 6
    const doc = new EventTarget();
    let title = '';
    Object.assign(doc, {
      body: { style: {} },
      documentElement: { style: {} },
      querySelector: sel => (sel === '#canvas' || sel === 'canvas') ? canvas : null,
      getElementById: id => id === 'canvas' ? canvas : null,
      createElement: tag => String(tag).toLowerCase() === 'canvas' ? cursorCanvas() : {},
      // no fullscreen, no pointer lock, always visible
      fullscreenElement: null,
      pointerLockElement: null,
      hidden: false,
      visibilityState: 'visible',
    });
    Object.defineProperty(doc, 'title', {
      get() { return title; },
      set(v) {
        v = style(v);
        if (v === title) return;
        title = v;
        post({ kind: 'title', title: v });
      },
    });
    globalThis.document = doc;

    // 7
    if (!navigator.userActivation)
      Object.defineProperty(navigator, 'userActivation', { configurable: true,
                                                           value: { isActive: false, hasBeenActive: false } });

    // The presents of SDL's framebuffer: putImageData of the whole window
    // on the 2D context of the canvas
    const put = OffscreenCanvasRenderingContext2D.prototype.putImageData;
    OffscreenCanvasRenderingContext2D.prototype.putImageData = function (image) {
      if (this.canvas !== canvas) return put.apply(this, arguments);
      const t = performance.now();
      try { return put.apply(this, arguments); } finally {
        const t1 = performance.now();
        stats.presents++;
        stats.presentMs += t1 - t;
        stats.pixels += image.width * image.height;
        if (stats.firstPresentMs === null) {
          stats.firstPresentMs = t1 - t0;
          post({ kind: 'painted', ms: stats.firstPresentMs });
        }
      }
    };

    // The fields of the page's records that become properties of the event
    const FIELDS = new Set(['clientX', 'clientY', 'screenX', 'screenY', 'pageX', 'pageY', 'offsetX', 'offsetY',
                            'button', 'buttons', 'movementX', 'movementY',
                            'ctrlKey', 'shiftKey', 'altKey', 'metaKey',
                            'deltaX', 'deltaY', 'deltaZ', 'deltaMode',
                            'key', 'code', 'keyCode', 'charCode', 'which', 'location', 'repeat', 'isComposing',
                            'timeStamp']);
    const TYPES = new Set(['mousemove', 'mousedown', 'mouseup', 'mouseenter', 'mouseleave', 'wheel',
                           'keydown', 'keyup', 'keypress']);

    return {
      attach(vm) {
        const M = vm.module;
        if (M.specialHTMLTargets && typeof M.specialHTMLTargets === 'object') M.specialHTMLTargets['#canvas'] = canvas;
        // the slices: how often the VM runs, and how long (the driver calls
        // M._vm_resume() for each one)
        const resume = M._vm_resume;
        M._vm_resume = function () {
          const t = performance.now();
          try { return resume.apply(this, arguments); } finally {
            stats.slices++;
            stats.sliceMs += performance.now() - t;
          }
        };
      },
      dispatch(rec) {
        if (!rec || !TYPES.has(rec.type)) return;
        const ev = new Event(rec.type, { cancelable: true });
        for (const k in rec) {
          if (!FIELDS.has(k)) continue;
          const v = rec[k];
          if (typeof v !== 'number' && typeof v !== 'boolean' && typeof v !== 'string') continue;
          Object.defineProperty(ev, k, { value: v });
        }
        (rec.type === 'mouseup' ? doc : canvas).dispatchEvent(ev);
      },
      stats() { return Object.assign({}, stats); },
    };
  }

  return { install, CURSOR_SCHEME };
});
