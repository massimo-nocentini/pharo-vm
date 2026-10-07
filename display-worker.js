// display-worker.js - the display of the Pharo world, in the VM's worker
//
// UMD: the global PharoDisplay in the worker (vm-worker.js loads it with
// importScripts when its init has a display), a CommonJS module in node
// (tests/wasm/world-harness.mjs).
//
//   const display = PharoDisplay.create(init.display, post);
//   ... createPharoVM({ ..., webDisplay: display })   // through vm-driver.js
//   display.handle({ kind: 'attach' }, vm);          // once the VM started
//   PharoDisplay.onMessage(message, vm);             // the page's "display"
//
// The display is the Module.webDisplay of the VM: the WebDisplayPlugin
// (src/emscripten/plugins/WebDisplayPlugin.c) calls its methods inside a
// slice, so they only draw and post, and never call into the VM:
//
//   open(width, height, title)     the world's window opens
//   present(pixels, width, height, rects, count)
//                                  paint the dirty rectangles of the frame:
//                                  width * height RGBA pixels at byte address
//                                  pixels of HEAPU8, count rectangles of 4
//                                  int32 (x, y, width, height) at byte address
//                                  rects; both valid during the call only
//   setTitle(title), setCursor(rgba, width, height, hotX, hotY),
//   setClipboard(text), focus()
//
// The canvas is init.canvas, the OffscreenCanvas that the page transferred,
// drawn with putImageData through an ImageData over HEAPU8, which is made
// again when the frame moves or changes size, or when a growth of the memory
// detached the old one.  The canvas takes the size of the frame, which the
// image makes devicePixelRatio times the size of the world in CSS pixels
// (its canvasScaleFactor), and the page shows it at that size divided by the
// ratio, one frame pixel per device pixel; what was drawn in a slice shows
// when the worker's task ends.  Without a canvas (node) the frame is
// copied into display.frame {width, height, data}, a memory framebuffer.
//
// The page sees the display through messages {type: 'display', kind, ...},
// posted with post(message, transfer):
//
//   displayOpen {width, height, title}
//   title {title}
//   cursor {rgba (an ArrayBuffer, transferred), width, height, hotX, hotY}
//   clipboardSet {text}           the image copied text
//   focus                         the image wants the keyboard
//   frame {frame, width, height, rects}   a present was painted (count)
//
// and sends its own, which onMessage handles between slices:
//
//   event {event}          an input record of 8 integers (see OSWebDriver)
//   resize {width, height, pixelWidth, pixelHeight}
//                          the canvas has that size now, in CSS pixels and in
//                          device pixels (the CSS ones when they are not
//                          given): the extents of the plugin, then a resize
//                          record of the CSS size
//   clipboard {text}       the text of a paste, before the key that pastes
//
// Each of them kicks the VM.  The display learns its VM, whose memory
// present() reads, from the first message that comes with it: attach, which
// the worker hands it once PharoVMDriver.start answered the VM, and which
// does nothing else, or else the page's first message (world.js sends the
// size of a canvas it shows at once, before the VM boots).  When the
// plugin's ring of events is full, the records wait here, and are pushed
// again a little later; mouse moves waiting there merge into the last one.

(function (root, factory) {
  if (typeof module === 'object' && module.exports) module.exports = factory();
  else root.PharoDisplay = factory();
})(typeof self !== 'undefined' ? self : this, function () {
  'use strict';

  // The types of the event records (OSWebDriver)
  const MOVE = 1, RESIZE = 8;
  const MAX_EXTENT = 16384;             // WebDisplayPlugin's largest canvas side
  const RETRY_MS = 16;                  // a full ring is tried again after this, a frame

  const now = () => (typeof performance !== 'undefined' ? performance.now() : Date.now());
  const clamp = (n, lo, hi) => Math.max(lo, Math.min(hi, Math.floor(Number(n) || 0)));

  let current = null;

  function create(init, post) {
    init = init || {};
    post = post || (() => {});
    const canvas = init.canvas || null;
    const ctx = canvas ? canvas.getContext('2d', { alpha: false }) : null;
    let vm = null, image = null, retry = 0;
    const backlog = [];
    const frame = canvas ? null : { width: 0, height: 0, data: new Uint8ClampedArray(0), rects: [] };
    // presents, the pixels they painted, when; events pushed, and pushes the
    // full ring refused
    const stats = { presents: 0, pixels: 0, firstPresentAt: 0, lastPresentAt: 0, events: 0, refused: 0 };

    const heap = () => vm && vm.module;

    // The ImageData over the frame in the memory of the VM
    function imageOf(M, pixels, width, height) {
      const buffer = M.HEAPU8.buffer;
      if (!image || image.data.buffer !== buffer || image.data.byteOffset !== pixels ||
          image.width !== width || image.height !== height || !image.data.length)
        image = new ImageData(new Uint8ClampedArray(buffer, pixels, width * height * 4), width, height);
      return image;
    }

    function paintCanvas(M, pixels, width, height, rects) {
      let full = false;
      if (canvas.width !== width || canvas.height !== height) {
        canvas.width = width;           // which clears it
        canvas.height = height;
        full = true;
      }
      const img = imageOf(M, pixels, width, height);
      if (full) ctx.putImageData(img, 0, 0);
      else for (const [x, y, w, h] of rects) ctx.putImageData(img, 0, 0, x, y, w, h);
    }

    function paintMemory(M, pixels, width, height, rects) {
      const src = M.HEAPU8;
      if (frame.width !== width || frame.height !== height) {
        frame.width = width;
        frame.height = height;
        frame.data = new Uint8ClampedArray(width * height * 4);
        rects = [[0, 0, width, height]];
      }
      for (const [x, y, w, h] of rects) {
        for (let row = y; row < y + h; row++) {
          const at = (row * width + x) * 4;
          frame.data.set(src.subarray(pixels + at, pixels + at + w * 4), at);
        }
      }
    }

    // Push the records waiting here, as long as the ring takes them
    function drain() {
      const M = heap();
      if (!M || vm.dead) { backlog.length = 0; return; }
      while (backlog.length) {
        const e = backlog[0];
        if (!M._webdisplay_push_event(e[0], e[1], e[2], e[3], e[4], e[5], e[6], e[7])) {
          stats.refused++;
          break;
        }
        backlog.shift();
        stats.events++;
      }
      if (backlog.length && !retry) {
        retry = setTimeout(() => {
          retry = 0;
          drain();
          if (vm && !vm.dead) vm.kick();
        }, RETRY_MS);
      }
    }

    function queue(record) {
      const e = new Array(8);
      for (let i = 0; i < 8; i++) e[i] = (Number(record[i]) || 0) | 0;
      const last = backlog[backlog.length - 1];
      if (last && last[0] === MOVE && e[0] === MOVE && last[5] === e[5]) backlog[backlog.length - 1] = e;
      else backlog.push(e);
    }

    const display = {
      // ---- Module.webDisplay, inside a slice

      open(width, height, title) {
        post({ type: 'display', kind: 'displayOpen', width, height, title });
      },

      present(pixels, width, height, rects, count) {
        const M = heap();
        if (!M) return;                 // no message yet told the VM
        const H = M.HEAP32, list = [];
        for (let i = 0, r = rects >>> 2; i < count; i++, r += 4) list.push([H[r], H[r + 1], H[r + 2], H[r + 3]]);
        if (canvas) paintCanvas(M, pixels, width, height, list);
        else {
          paintMemory(M, pixels, width, height, list);
          frame.rects = list;
        }
        const t = now();
        stats.presents++;
        for (const r of list) stats.pixels += r[2] * r[3];
        if (!stats.firstPresentAt) stats.firstPresentAt = t;
        stats.lastPresentAt = t;
        post({ type: 'display', kind: 'frame', frame: stats.presents, width, height, rects: count });
      },

      setTitle(title) {
        post({ type: 'display', kind: 'title', title });
      },

      setCursor(rgba, width, height, hotX, hotY) {
        post({ type: 'display', kind: 'cursor', rgba: rgba.buffer, width, height, hotX, hotY }, [rgba.buffer]);
      },

      setClipboard(text) {
        post({ type: 'display', kind: 'clipboardSet', text });
      },

      focus() {
        post({ type: 'display', kind: 'focus' });
      },

      // ---- the page's messages, between slices

      handle(m, driver) {
        if (driver) vm = driver;
        const M = heap();
        if (!M || vm.dead) return;
        switch (m.kind) {
        case 'attach':                  // the VM only
          return;
        case 'event':
          queue(m.event || []);
          break;
        case 'resize': {
          const width = clamp(m.width, 0, MAX_EXTENT), height = clamp(m.height, 0, MAX_EXTENT);
          M._webdisplay_set_extent(width, height);
          M._webdisplay_set_pixel_extent(clamp(m.pixelWidth == null ? width : m.pixelWidth, 0, MAX_EXTENT),
                                         clamp(m.pixelHeight == null ? height : m.pixelHeight, 0, MAX_EXTENT));
          queue([RESIZE, now(), 0, 0, width, 0, height, 0]);
          break;
        }
        case 'clipboard': {
          const bytes = new TextEncoder().encode(String(m.text || ''));
          const p = Number(M._webdisplay_clipboard_buffer(bytes.length));
          if (p) {
            M.HEAPU8.set(bytes, p);     // HEAPU8 after the call: it may have grown the memory
            M._webdisplay_clipboard_commit(bytes.length);
          }
          break;
        }
        default:
          return;
        }
        drain();
        vm.kick();
      },

      frame, stats,
      get backlog() { return backlog.length; },
    };
    current = display;
    return display;
  }

  // vm-worker.js passes the page's display messages here, with the driver
  function onMessage(m, vm) {
    if (current) current.handle(m, vm);
  }

  return { create, onMessage };
});
