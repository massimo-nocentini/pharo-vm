// web-socket.js - --pre-js of both Pharo VMs on wasm: the WebSocket of the host for the WebSocketPlugin
//
// PharoWebSocket.create({signal(index), network(), WebSocket(), isBrowser,
// siteHostname, setTimeout, clearTimeout}) answers the WebSocket client
// connections of a VM, Module.webSocket; the primitives of
// src/emscripten/plugins/WebSocketPlugin.c call its methods.  WebSocket()
// answers the constructor at each open (globalThis.WebSocket: the guard of
// vm-worker.js, or a fake of the tests); isBrowser and siteHostname refuse
// the hosts of the site's domain (its cookies would go with the handshake,
// see ofTheSite); every
// timer is made with the setTimeout given, and unref()'d when it can be (in
// node: no timer of the service keeps node alive).  Linked after
// host-events.js, and a plain script as that one: a node test evaluates it
// alone.  It declares no binding named WebSocket: Emscripten's sockets
// (libsockfs.js) take the constructor by that unqualified name in the scope
// of the factory, where this file is.
//
// A socket goes through
//
//   1 CONNECTING  the handshake goes on                               -> 2, 3, 5
//   2 OPEN        messages come and go                                -> 3, 4, 5
//   3 CLOSING     the image asked to close, or the server sent its
//                 close frame (seen through readyState: no event)     -> 4, 5
//   4 CLOSED      the close event came, clean or not, or the close
//                 timer expired; the messages queued stay readable
//   5 FAILED      the info record is the error's; the messages queued
//                 stay readable
//
// and the events and the timers move it so, and nothing else does:
//
//   in   open  message           error         close event   handshake  close timer  networkOff()
//   1    2     -                 5 network     5 network     5 timeout               5 off
//   2          queued, or 5      ignored       4                                     5 off
//              tooLarge/overflow
//   3    -     the same          ignored       4 (*)                    4: 1006,     5 off
//                                                                       timed out
//   4,5  ignored
//
// (*) 1006 'closed before it was open', not clean, when the close began in
// state 1.  A close before the WebSocket was made (in the slice of the open)
// makes none: state 4 the same way, from a microtask.  Entering 4 or 5
// clears the timers of the socket.  A socket fails with tooLarge (closed
// with 4009 'message too large'), overflow (4008 'too many unread
// messages') and off (1000 'network access is off'), or timeout (closed).
//
// Three semaphores per socket, signalled between the slices, never inside a
// method, and only while the image has the socket (the rule of
// host-events.js):
//
//   entering 2, 4 or 5                                  state, read, write
//   the queue goes from empty to one message            read
//   bufferedAmount after a send crosses LOW_WATER
//   downward, or reaches 0, or readyState is 2 or more  write
//
// Entering 3 signals nothing: from the image it happens inside close(), from
// the server it is no event; the close timer makes it 4 within
// CLOSE_TIMEOUT.  The timers: the handshake timer of a socket (timeoutMs, 0
// none), cleared on 2; its close timer (CLOSE_TIMEOUT), armed by close() and
// when state() or the ticker first sees readyState 2 in state 2; and one
// ticker for the service, a chain of timeouts that runs while a socket
// drains (bufferedAmount above 0 after a send) or is open: every 16 ms while
// a draining socket made progress in the last second, 250 ms while none did
// (a deaf peer), 500 ms while it only watches the open ones for readyState 2.
//
// A socket that the image forgets (release, shutdown) whose WebSocket has
// not had its close event is lingering until that event: open() counts it
// towards MAX_OPEN, so a loop of opens and releases to a deaf server is
// bounded.
//
// Node 24 and 25 fire the error and the close events of a WebSocket that is
// still connecting inside its close(), and leave readyState at 2 after them.
// So the events that come inside a close() of the service wait for a
// microtask (nothing signals inside a method), and a WebSocket whose close
// event came never lingers, whatever its readyState says.
//
// The methods are synchronous, never throw and never call the VM:
//
//   availability()   0 no WebSocket here, 1 ready, 2 the network is off
//   open(semState, semRead, semWrite, record, maxMessage, maxQueued, timeoutMs)
//                    the id, or 0 when the availability is not 1, after
//                    shutdown(), or when the sockets plus the lingering ones
//                    are MAX_OPEN; record (a Uint8Array that becomes the
//                    socket's) is checked, and the WebSocket made, after a
//                    microtask: a record that fails the checks makes the
//                    state 5 request there
//   state(id)        state + 8 * the number of messages queued; -1 for an
//                    unknown id.  In state 2 it reads readyState: 2 there
//                    makes the state 3, with the close timer armed
//   infoSize(id)     the size of the info record, -1 in state 1 and for an
//                    unknown id
//   info(id, dest)   copies the info record into the Uint8Array dest
//   peek(id)         -1 when no message is queued, else 2 * its size in
//                    bytes, + 1 for text
//   receive(id, dest)  copies the next message into dest, whose length is
//                    its size, and drops it; answers its size, -1 for an
//                    unknown id, none queued, or a dest of another size
//   drop(id)         drops the next message without copying it; answers its
//                    size, or -1; a socket in 1, 2 or 3 then fails with
//                    tooLarge (the image cannot hold it), closed with 4009
//   send(id, bytes, isText)
//                    bufferedAmount after the send (>= 0); -1 unknown id;
//                    -2 readyState is not 1, or the socket is over; -3 the
//                    send buffer is full, nothing was sent; -4 the bytes are
//                    not UTF-8 (text) or more than MAX_SEND; -5 the copy
//                    failed.  bytes is a view of the heap: binary is copied,
//                    text decoded from it (a leading BOM kept)
//   buffered(id)     bufferedAmount, -1 unknown id
//   close(id, code, reason)
//                    1 when it started the closing handshake (state 3),
//                    also in state 1; 0 in state 3, 4 or 5; -1 unknown id;
//                    -2 the reason (a Uint8Array) is not UTF-8, or the
//                    WebSocket refused the code or the reason
//   release(id)      1 when the id was known, else 0: forgets the socket at
//                    once (no signal any more); its WebSocket, unless
//                    CLOSED, is closed with 1000 'released' (unless closing)
//                    and lingers
//   lingering()      the number of forgotten WebSockets without their close
//                    event
//   networkOff()     every socket in 1, 2 or 3 fails with kind off, its
//                    queue kept, and is closed with 1000
//   shutdown()       every socket is forgotten, its WebSocket closed with
//                    1000 'the VM ended' (unless closing) and lingering;
//                    every timer cleared; nothing signals; open() answers 0
//                    after it
//
// The records are UTF-8, with a LF between their lines; in those of the
// host a CR, a LF or a NUL in a part becomes a space:
//
//   open      URL, then one offered subprotocol per line.  A CR or a NUL
//             in it, a URL that is not an absolute ws: or wss: one, or has
//             a user name, a password or a fragment, in a browser one of a
//             host of the site's domain, a subprotocol that is no token or
//             offered twice (ignoring case) are refused (kind request),
//             never changed
//   info      in 2 and 3: chosen protocol, extensions, URL; in 4 the same,
//             then the close code, the reason, 1 when clean or 0
//   error     in 5: kind, message, chosen protocol, extensions, URL (empty
//             when it never opened); the kinds are request, network,
//             timeout, off, tooLarge and overflow

var PharoWebSocket = (() => {
  const CONNECTING = 1, OPEN = 2, CLOSING = 3, CLOSED = 4, FAILED = 5;
  const STATE = 0, READ = 1, WRITE = 2;
  const MAX_OPEN = 32, MAX_MESSAGES = 65536;
  const MiB = 1024 * 1024;
  const MAX_SEND = 64 * MiB, HIGH_WATER = 16 * MiB, LOW_WATER = MiB, MAX_QUEUED_TOTAL = 256 * MiB;
  const CLOSE_TIMEOUT = 5000;
  const FAST = 16, SLOW = 250, WATCH = 500, PROGRESS = 1000;
  const TOKEN = /^[!#$%&'*+\-.^_`|~0-9A-Za-z]+$/;
  const encoder = new TextEncoder();
  const oneLine = s => String(s).replace(/[\r\n\0]/g, ' ');
  const record = lines => encoder.encode(lines.map(oneLine).join('\n'));
  // fatal, and a leading U+FEFF is the image's: kept
  const decoder = new TextDecoder('utf-8', { fatal: true, ignoreBOM: true });
  const HANDSHAKE = 'the handshake failed (no answer, a refusal, or a status other than 101)';

  // Whether the cookies of a site of that hostname may go to host: the same
  // host, a parent or a subdomain of it, or one with the same last two
  // labels.  A cookie with a Domain attribute goes to every host of its
  // registrable domain, which has two labels or more; without the list of
  // public suffixes the last two stand for it, which refuses too much under
  // a suffix of two labels (co.uk, github.io).  An IP address only matches
  // itself.
  function ofTheSite(host, site) {
    const h = host.replace(/\.$/, ''), s = site.replace(/\.$/, '');
    if (!h || !s) return false;
    if (h === s) return true;
    const ip = x => x.startsWith('[') || /^[0-9.]+$/.test(x);
    if (ip(h) || ip(s)) return false;
    if (h.endsWith('.' + s) || s.endsWith('.' + h)) return true;
    const last2 = x => x.split('.').slice(-2).join('.');
    return h.includes('.') && s.includes('.') && last2(h) === last2(s);
  }

  // What went wrong; node tells the cause of an exception, a browser does not
  function describe(e) {
    const message = String((e && e.message) || e), cause = e && e.cause;
    if (!cause) return message;
    const why = [cause.code, cause.message].filter(s => s).join(': ');
    return why ? message + ' (' + why + ')' : message;
  }
  // What an error event says, which is little: nothing in node 24 and 25
  function eventMessage(ev) {
    const parts = [HANDSHAKE];
    if (ev && ev.message) parts.push(String(ev.message));
    const code = ev && ev.error && ev.error.cause && ev.error.cause.code;
    if (code) parts.push(String(code));
    return parts.join(': ');
  }

  function create({ signal, network = () => true, WebSocket: constructorOf = () => undefined, isBrowser = false,
                    siteHostname = '', setTimeout: setTimer = setTimeout, clearTimeout: clearTimer = clearTimeout,
                    maxQueuedTotal = MAX_QUEUED_TOTAL }) {
    const open = new Map();
    const lingering = new Set();
    let last = 1 + Math.floor(Math.random() * 0x7ffffffe);
    let queuedTotal = 0, gone = false;
    let ticker = null, tickerDelay = 0;
    // > 0 inside a close() of a WebSocket, where node fires its events
    let inside = 0;

    const later = (f, ms) => {
      const t = setTimer(f, ms);
      if (t && typeof t.unref === 'function') t.unref();
      return t;
    };
    function nextId() {
      do last = last >= 0x7fffffff ? 1 : last + 1; while (open.has(last));
      return last;
    }
    function tell(r, ...which) {
      for (const w of which) if (open.get(r.id) === r) signal(r.sems[w]);
    }
    function stopTimers(r) {
      if (r.handshakeTimer) clearTimer(r.handshakeTimer);
      if (r.closeTimer) clearTimer(r.closeTimer);
      r.handshakeTimer = r.closeTimer = null;
      r.draining = false;
    }
    function armClose(r) {
      if (r.closeTimer) return;
      r.closeTimer = later(() => {
        r.closeTimer = null;
        if (r.state === CLOSING) closed(r, 1006, 'closing handshake timed out', false);
      }, CLOSE_TIMEOUT);
    }
    function closeWs(ws, code, reason) {
      inside++;
      try { ws.close(code, reason); } finally { inside--; }
    }
    // an event of a WebSocket, after a microtask when it came inside a close()
    function deliver(f) {
      if (inside) Promise.resolve().then(f); else f();
    }
    // the WebSocket, unless closing or closed already
    function shut(r, code, reason) {
      const ws = r.ws;
      if (!ws || r.ended || ws.readyState >= 2) return;
      try { closeWs(ws, code, reason); } catch (e) { try { closeWs(ws); } catch (e2) { /* closing */ } }
    }
    function urlOf(r) { return r.ws && r.ws.url ? r.ws.url : r.url; }
    function fail(r, kind, message, code, reason) {
      if (r.state >= CLOSED) return;
      r.state = FAILED;
      r.info = record([kind, message, r.protocol, r.extensions, r.opened ? urlOf(r) : '']);
      stopTimers(r);
      if (code !== undefined) shut(r, code, reason);
      tell(r, STATE, READ, WRITE);
    }
    function closed(r, code, reason, clean) {
      if (r.state >= CLOSED) return;
      r.state = CLOSED;
      r.info = record([r.protocol, r.extensions, urlOf(r), code, reason, clean ? 1 : 0]);
      stopTimers(r);
      tell(r, STATE, READ, WRITE);
    }
    // the server sent its close frame: no event says so
    function serverClosing(r) {
      r.state = CLOSING;
      armClose(r);
    }
    function forget(r, reason) {
      open.delete(r.id);
      stopTimers(r);
      queuedTotal -= r.queuedBytes;
      r.queue = [];
      r.first = r.count = r.queuedBytes = 0;
      r.forgotten = true;
      const ws = r.ws;
      if (ws && !r.ended && ws.readyState !== 3) {
        shut(r, 1000, reason);
        // (its close event may have come inside that close())
        if (!r.ended) lingering.add(ws);
      }
    }

    // ---- the ticker
    function wanted() {
      let d = 0;
      for (const r of open.values()) {
        const want = r.draining ? (r.idle < PROGRESS ? FAST : SLOW) : r.state === OPEN ? WATCH : 0;
        if (want && (!d || want < d)) d = want;
      }
      return d;
    }
    function tick() {
      ticker = null;
      const dt = tickerDelay;
      for (const r of [...open.values()]) {
        if (r.state === OPEN && r.ws.readyState >= 2) serverClosing(r);
        if (!r.draining) continue;
        const b = r.ws.bufferedAmount;
        let wake = r.lastBuffered > LOW_WATER && b <= LOW_WATER;
        if (b < r.lastBuffered) r.idle = 0; else r.idle += dt;
        r.lastBuffered = b;
        if (b === 0 || r.ws.readyState >= 2) {
          r.draining = false;
          wake = true;
        }
        if (wake) tell(r, WRITE);
      }
      schedule();
    }
    function schedule() {
      const d = gone ? 0 : wanted();
      if (ticker && (!d || d < tickerDelay)) {
        clearTimer(ticker);
        ticker = null;
      }
      if (!d || ticker) return;
      tickerDelay = d;
      ticker = later(tick, d);
    }

    // ---- the events of a WebSocket
    function onOpen(r, ws) {
      if (r.ws !== ws || r.state !== CONNECTING) return;
      if (r.handshakeTimer) clearTimer(r.handshakeTimer);
      r.handshakeTimer = null;
      r.opened = true;
      r.protocol = ws.protocol || '';
      r.extensions = ws.extensions || '';
      r.state = OPEN;
      tell(r, STATE, READ, WRITE);
      schedule();
    }
    function onMessage(r, ws, data) {
      if (r.ws !== ws || (r.state !== OPEN && r.state !== CLOSING)) return;
      let bytes, text;
      if (typeof data === 'string') {
        // UTF-8 never has fewer bytes than UTF-16 units: no need to encode
        if (data.length > r.maxMessage) return tooLarge(r);
        bytes = encoder.encode(data);
        text = true;
      } else if (data && typeof data.byteLength === 'number') {
        bytes = new Uint8Array(data);
        text = false;
      } else {
        fail(r, 'network', 'a message that is neither text nor an ArrayBuffer', 1003, 'unsupported data');
        return;
      }
      if (bytes.length > r.maxMessage) return tooLarge(r);
      const n = bytes.length;
      if (r.count >= MAX_MESSAGES || (r.count > 0 && r.queuedBytes + n > r.maxQueued)) {
        fail(r, 'overflow', 'more than ' + r.maxQueued + ' bytes or ' + MAX_MESSAGES + ' messages unread', 4008, 'too many unread messages');
        return;
      }
      if (queuedTotal + n > maxQueuedTotal) {
        fail(r, 'overflow', 'the queues of all sockets are full', 4008, 'too many unread messages');
        return;
      }
      r.queue.push({ bytes, text });
      r.count++;
      r.queuedBytes += n;
      queuedTotal += n;
      if (r.count === 1) tell(r, READ);
    }
    function tooLarge(r) {
      fail(r, 'tooLarge', 'a message of more than ' + r.maxMessage + ' bytes', 4009, 'message too large');
    }
    function onError(r, ws, ev) {
      if (r.ws !== ws || r.state !== CONNECTING) return;
      fail(r, 'network', eventMessage(ev));
    }
    function onClose(r, ws, ev) {
      if (r.ws !== ws || r.forgotten) return;
      if (r.state === CONNECTING) fail(r, 'network', eventMessage(ev));
      else if (r.state === CLOSING && r.closedConnecting) closed(r, 1006, 'closed before it was open', false);
      else if (r.state === OPEN || r.state === CLOSING) closed(r, ev.code, ev.reason || '', !!ev.wasClean);
    }

    // the open record, checked, and the WebSocket, made out of the primitive
    function connect(r, bytes) {
      if (r.forgotten || r.state !== CONNECTING) return;
      let lines;
      try { lines = decoder.decode(bytes).split('\n'); }
      catch (e) { fail(r, 'request', 'the open record is not UTF-8'); return; }
      const address = lines[0], protocols = lines.slice(1);
      let url;
      try {
        if (/[\r\0]/.test(address)) throw new TypeError('the URL has a CR, a LF or a NUL');
        url = new URL(address);
        if (url.protocol !== 'ws:' && url.protocol !== 'wss:') throw new TypeError('not a ws: or wss: URL: ' + url.protocol);
        if (url.username || url.password) throw new TypeError('the URL has a user name or a password');
        if (url.href.includes('#')) throw new TypeError('the URL has a fragment');
        const seen = new Set();
        for (const p of protocols) {
          if (!TOKEN.test(p)) throw new TypeError('not a subprotocol: ' + JSON.stringify(p));
          if (seen.has(p.toLowerCase())) throw new TypeError('the subprotocol ' + p + ' is offered twice');
          seen.add(p.toLowerCase());
        }
        if (isBrowser && siteHostname && ofTheSite(url.hostname, siteHostname))
          throw new TypeError('a host of the domain of this site is refused: its cookies would be sent');
      } catch (e) {
        fail(r, 'request', describe(e));
        return;
      }
      r.url = url.href;
      if (!network()) { fail(r, 'off', 'network access is off'); return; }
      const Ctor = constructorOf();
      if (typeof Ctor !== 'function') { fail(r, 'network', 'no WebSocket here'); return; }
      let ws;
      try {
        ws = new Ctor(url.href, protocols);
      } catch (e) {
        if (e && e.name === 'SecurityError' && !network()) fail(r, 'off', 'network access is off');
        else if (e && e.name === 'SecurityError') fail(r, 'request', 'not allowed from this page (an https: page reaches only wss:): ' + e.name + ': ' + describe(e));
        else fail(r, 'request', (e && e.name ? e.name + ': ' : '') + describe(e));
        return;
      }
      r.ws = ws;
      ws.binaryType = 'arraybuffer';
      ws.addEventListener('open', () => deliver(() => onOpen(r, ws)));
      ws.addEventListener('message', ev => deliver(() => onMessage(r, ws, ev.data)));
      ws.addEventListener('error', ev => deliver(() => onError(r, ws, ev)));
      ws.addEventListener('close', ev => {
        r.ended = true;
        lingering.delete(ws);
        deliver(() => onClose(r, ws, ev));
      });
    }

    function availability() {
      if (typeof constructorOf() !== 'function') return 0;
      return network() ? 1 : 2;
    }
    function infoOf(r) {
      if (r.state === OPEN || r.state === CLOSING) return record([r.protocol, r.extensions, urlOf(r)]);
      return r.info;
    }
    function head(r) { return r.count ? r.queue[r.first] : null; }
    function dequeue(r) {
      const m = r.queue[r.first];
      r.queue[r.first++] = null;
      r.count--;
      r.queuedBytes -= m.bytes.length;
      queuedTotal -= m.bytes.length;
      // not a shift() of each: that moves all of a long queue
      if (!r.count || r.first === 1024) {
        r.queue.splice(0, r.first);
        r.first = 0;
      }
      return m;
    }
    function current(r) {
      if (r.state === OPEN && r.ws && r.ws.readyState >= 2) serverClosing(r);
      return r.state;
    }

    return {
      availability,
      open(semState, semRead, semWrite, bytes, maxMessage, maxQueued, timeoutMs) {
        if (gone || open.size + lingering.size >= MAX_OPEN || availability() !== 1) return 0;
        const r = { id: nextId(), sems: [semState, semRead, semWrite], state: CONNECTING, ws: null, url: '',
          protocol: '', extensions: '', opened: false, info: null, forgotten: false, closedConnecting: false, ended: false,
          queue: [], first: 0, count: 0, queuedBytes: 0, maxMessage, maxQueued,
          handshakeTimer: null, closeTimer: null, draining: false, lastBuffered: 0, idle: 0 };
        open.set(r.id, r);
        if (timeoutMs > 0) r.handshakeTimer = later(() => {
          r.handshakeTimer = null;
          if (r.state === CONNECTING) fail(r, 'timeout', 'not open after ' + timeoutMs + ' ms', 1000, 'timeout');
        }, timeoutMs);
        Promise.resolve().then(() => connect(r, bytes));
        return r.id;
      },
      state(id) {
        const r = open.get(id);
        return r ? current(r) + 8 * r.count : -1;
      },
      infoSize(id) {
        const r = open.get(id);
        const info = r && infoOf(r);
        return info ? info.length : -1;
      },
      info(id, dest) {
        const r = open.get(id);
        const info = r && infoOf(r);
        if (!info) return -1;
        const n = Math.min(dest.length, info.length);
        dest.set(info.subarray(0, n));
        return n;
      },
      peek(id) {
        const r = open.get(id);
        const m = r && head(r);
        return m ? 2 * m.bytes.length + (m.text ? 1 : 0) : -1;
      },
      receive(id, dest) {
        const r = open.get(id);
        const m = r && head(r);
        if (!m || dest.length !== m.bytes.length) return -1;
        dest.set(m.bytes);
        dequeue(r);
        return m.bytes.length;
      },
      drop(id) {
        const r = open.get(id);
        if (!r || !head(r)) return -1;
        const n = dequeue(r).bytes.length;
        if (r.state <= CLOSING) fail(r, 'tooLarge', 'a message of ' + n + ' bytes is larger than the image can hold', 4009, 'message too large');
        return n;
      },
      send(id, bytes, isText) {
        const r = open.get(id);
        if (!r) return -1;
        const ws = r.ws;
        if (!ws || ws.readyState !== 1 || current(r) !== OPEN) return -2;
        if (bytes.length > MAX_SEND) return -4;
        const before = ws.bufferedAmount;
        if (before > 0 && before + bytes.length > HIGH_WATER) return -3;
        let data;
        if (isText) {
          try { data = decoder.decode(bytes); } catch (e) { return -4; }
        } else {
          try { data = bytes.slice(); } catch (e) { return -5; }
        }
        try { ws.send(data); } catch (e) { return -2; }
        const after = ws.bufferedAmount;
        if (after > 0) {
          // (what the ticker saw last, unless the send put more on it)
          r.lastBuffered = r.draining ? Math.max(r.lastBuffered, after) : after;
          if (!r.draining) r.idle = 0;
          r.draining = true;
          schedule();
        }
        return after;
      },
      buffered(id) {
        const r = open.get(id);
        if (!r) return -1;
        return r.ws ? r.ws.bufferedAmount : 0;
      },
      close(id, code, reasonBytes) {
        const r = open.get(id);
        if (!r) return -1;
        let reason;
        try { reason = decoder.decode(reasonBytes); } catch (e) { return -2; }
        if (r.state >= CLOSING) return 0;
        if (r.state === OPEN && r.ws.readyState >= 2) {
          serverClosing(r);
          return 0;
        }
        if (r.ws) {
          try { closeWs(r.ws, code, reason); } catch (e) { return -2; }
        }
        if (r.state === CONNECTING) {
          r.closedConnecting = true;
          if (!r.ws) {
            r.state = CLOSING;
            Promise.resolve().then(() => closed(r, 1006, 'closed before it was open', false));
            return 1;
          }
        }
        r.state = CLOSING;
        if (r.handshakeTimer) clearTimer(r.handshakeTimer);
        r.handshakeTimer = null;
        armClose(r);
        return 1;
      },
      release(id) {
        const r = open.get(id);
        if (!r) return 0;
        forget(r, 'released');
        schedule();
        return 1;
      },
      lingering() { return lingering.size; },
      networkOff() {
        for (const r of [...open.values()]) {
          if (r.state <= CLOSING) fail(r, 'off', 'network access is off', 1000, 'network access is off');
        }
        schedule();
      },
      shutdown() {
        gone = true;
        for (const r of [...open.values()]) forget(r, 'the VM ended');
        if (ticker) clearTimer(ticker);
        ticker = null;
      },
    };
  }

  return { create };
})();

if (typeof Module === 'object') {
  Module['webSocket'] = PharoWebSocket.create({
    signal: index => Module['pharoHost'].signal(index),
    network: () => Module['pharoHost'].network,
    WebSocket: () => typeof globalThis.WebSocket === 'function' ? globalThis.WebSocket : undefined,
    isBrowser: !(typeof process === 'object' && process.versions && process.versions.node),
    // (the worker sets it: self.location.hostname)
    siteHostname: String(Module['siteHostname'] || ''),
    setTimeout: (f, ms) => setTimeout(f, ms),
    clearTimeout: t => clearTimeout(t),
  });
  Module['pharoHost'].add(Module['webSocket']);
}
