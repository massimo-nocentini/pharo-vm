// web-fetch.js - --pre-js of both Pharo VMs on wasm: fetch for the WebFetchPlugin
//
// PharoWebFetch.create({signal(index), network(), fetch, isBrowser}) answers
// the table of the requests of a VM, Module.webFetch; the primitives of
// src/emscripten/plugins/WebFetchPlugin.c call its methods.  Linked after
// host-events.js, and a plain script as that one: a node test evaluates it
// alone.
//
// A request goes through
//
//   1 PENDING  no answer yet                                     -> 2, 4
//   2 HEAD     status and headers; size bytes of the body are
//              ready, and the body goes on                       -> 3, 4
//   3 DONE     all of the body is here, size bytes unread
//   4 FAILED   head is the error record; the bytes ready are dropped
//
// and signals its semaphore at each of these, when size goes from 0 to more,
// and when it reaches window, until it is closed (the rule of host-events.js).
// The body waits in the JavaScript heap until read(); at window bytes the
// stream is no longer read.  Nothing signals inside start() or close().
//
// The methods are synchronous, never throw and never call the VM:
//
//   availability()   0 no fetch here, 1 ready, 2 the network is off
//   start(sem, text, body, window, timeoutMs, flags)
//                    the id, or 0 when MAX_OPEN requests are open (or the
//                    availability is not 1).  text is the request record;
//                    body a Uint8Array that becomes the request's, or null;
//                    timeoutMs 0 is none; flags 1 does not follow
//                    redirects, flags 2 is the cache mode no-store
//   state(id)        state + 8 * size, or -1 for an unknown id, as below
//   headSize(id)     the size of the record of the answer, or of the error;
//                    -1 in state 1
//   head(id, dest)   copies the record into the Uint8Array dest
//   read(id, dest)   copies at most dest.length bytes of the body; answers
//                    how many
//   close(id)        1, or 0 for an unknown id
//   networkOff()     every request that is not over fails, with kind off
//   shutdown()       every request is aborted and forgotten
//
// The records are UTF-8, with a LF between their lines:
//
//   request   method, URL, then 'name: value' lines
//   answer    status, status text, final URL, type, 1 when redirected or 0,
//             then 'name: value' lines
//   error     kind, message; the kinds are request (it is not valid),
//             network (no answer), body (the answer broke off), timeout,
//             redirect (a browser does not show it), aborted and off

var PharoWebFetch = (() => {
  const PENDING = 1, HEAD = 2, DONE = 3, FAILED = 4;
  const MAX_OPEN = 64;
  // the headers of a request that are not the image's: those that browsers
  // forbid, dropped in node too
  const DROPPED = new Set(['accept-charset', 'accept-encoding', 'access-control-request-headers',
    'access-control-request-method', 'connection', 'content-length', 'cookie', 'cookie2', 'date', 'dnt',
    'expect', 'host', 'keep-alive', 'origin', 'referer', 'set-cookie', 'te', 'trailer',
    'transfer-encoding', 'upgrade', 'via']);
  const encoder = new TextEncoder();
  const oneLine = s => String(s).replace(/[\r\n\0]/g, ' ');
  const record = lines => encoder.encode(lines.map(oneLine).join('\n'));

  // What went wrong; node tells the cause, a browser does not
  function describe(e) {
    const message = String((e && e.message) || e), cause = e && e.cause;
    if (!cause) return message;
    const why = [cause.code, cause.message].filter(s => s).join(': ');
    return why ? message + ' (' + why + ')' : message;
  }

  function create({ signal, network = () => true, fetch, isBrowser = false }) {
    const open = new Map();
    let last = 1 + Math.floor(Math.random() * 0x7ffffffe);

    function nextId() {
      do last = last >= 0x7fffffff ? 1 : last + 1; while (open.has(last));
      return last;
    }
    function tell(r) { if (open.get(r.id) === r) signal(r.sem); }
    // the request is over: no timer, and the loop in run() goes on to its end
    function settle(r) {
      clearTimeout(r.timer);
      r.timer = 0;
      release(r);
    }
    function release(r) {
      const f = r.more;
      r.more = null;
      if (f) f();
    }
    function drop(r) {
      r.chunks = [];
      r.first = r.at = r.size = 0;
    }
    function fail(r, kind, message) {
      if (r.state >= DONE) return;
      r.state = FAILED;
      r.head = record([kind, message]);
      drop(r);
      settle(r);
      tell(r);
      r.control.abort();
    }
    function forget(r) {
      const going = r.state < DONE;
      open.delete(r.id);
      r.state = FAILED;
      r.head = null;
      drop(r);
      settle(r);
      if (going) r.control.abort();
    }

    // The URL and the headers of the image, checked as fetch would: throws
    function check(text, body, flags) {
      const [method, address, ...lines] = text.split('\n');
      const url = new URL(address);
      if (url.protocol !== 'http:' && url.protocol !== 'https:')
        throw new TypeError('not an http: or https: URL: ' + url.protocol);
      if (url.username || url.password) throw new TypeError('the URL has a user name or a password');
      const headers = [];
      for (const line of lines) {
        const i = line.indexOf(':');
        if (i < 1) throw new TypeError('not a header: ' + line);
        const name = line.slice(0, i), key = name.toLowerCase();
        if (DROPPED.has(key) || /^(proxy|sec)-/.test(key) || (isBrowser && key === 'user-agent')) continue;
        // only spaces and tabs, as fetch does: trim() also removes U+00A0,
        // the byte A0 of a value sent as UTF-8 one character a byte (the
        // end of an 'à' in a Last-Event-ID)
        headers.push([name, line.slice(i + 1).replace(/^[\t ]+|[\t ]+$/g, '')]);
      }
      const init = { method: method.toUpperCase(), headers };
      new Request(url.href, Object.assign({ body: body ? new Uint8Array(1) : undefined }, init));
      return { url: url.href, init: Object.assign(init, {
        body: body || undefined,
        mode: 'cors',
        credentials: 'omit',
        referrerPolicy: 'no-referrer',
        redirect: flags & 1 ? 'manual' : 'follow',
        cache: flags & 2 ? 'no-store' : 'default' }) };
    }

    async function run(r, text, body, flags) {
      // nothing happens inside the primitive that starts the request
      await null;
      try {
        if (r.state !== PENDING) return;
        let request;
        try { request = check(text, body, flags); }
        catch (e) { fail(r, 'request', describe(e)); return; }
        request.init.signal = r.control.signal;
        text = body = null;
        const response = await fetch(request.url, request.init);
        request = null;
        if (r.state !== PENDING) return;
        if (response.type === 'opaqueredirect') {
          fail(r, 'redirect', 'the server redirects, and a browser does not say where');
          return;
        }
        const lines = [response.status, response.statusText, response.url, response.type, response.redirected ? 1 : 0];
        response.headers.forEach((value, name) => { lines.push(name + ': ' + value); });
        r.head = record(lines);
        r.state = HEAD;
        tell(r);
        if (response.body) await readBody(r, response.body.getReader());
        if (r.state !== HEAD) return;
        r.state = DONE;
        settle(r);
        tell(r);
      } catch (e) {
        fail(r, e && e.name === 'AbortError' ? 'aborted' : r.state === PENDING ? 'network' : 'body', describe(e));
      }
    }
    // to the end of the body, or of the request
    async function readBody(r, reader) {
      for (;;) {
        while (r.state === HEAD && r.size >= r.window) await new Promise(f => { r.more = f; });
        if (r.state !== HEAD) return;
        const { done, value } = await reader.read();
        if (r.state !== HEAD || done) return;
        if (!value.length) continue;
        r.chunks.push(value);
        r.size += value.length;
        // the first bytes; or all that come before a read(): the one who
        // waits for the end of the body must know that it does not end
        if (r.size === value.length || r.size >= r.window) tell(r);
      }
    }

    function availability() {
      if (typeof fetch !== 'function' || typeof AbortController !== 'function') return 0;
      return network() ? 1 : 2;
    }

    return {
      availability,
      start(sem, text, body, window, timeoutMs, flags) {
        if (open.size >= MAX_OPEN || availability() !== 1) return 0;
        const r = { id: nextId(), sem, state: PENDING, head: null,
          chunks: [], first: 0, at: 0, size: 0,         // the first unread byte is chunks[first][at]
          window, more: null, control: new AbortController(), timer: 0 };
        open.set(r.id, r);
        if (timeoutMs > 0)
          r.timer = setTimeout(() => fail(r, 'timeout', 'not complete after ' + timeoutMs + ' ms'), timeoutMs);
        run(r, text, body, flags);
        return r.id;
      },
      state(id) {
        const r = open.get(id);
        return r ? r.state + 8 * r.size : -1;
      },
      headSize(id) {
        const r = open.get(id);
        return r && r.head ? r.head.length : -1;
      },
      head(id, dest) {
        const r = open.get(id);
        if (!r || !r.head) return -1;
        const n = Math.min(dest.length, r.head.length);
        dest.set(r.head.subarray(0, n));
        return n;
      },
      read(id, dest) {
        const r = open.get(id);
        if (!r) return -1;
        let n = 0;
        while (n < dest.length && r.size) {
          const chunk = r.chunks[r.first], k = Math.min(dest.length - n, chunk.length - r.at);
          dest.set(chunk.subarray(r.at, r.at + k), n);
          n += k;
          r.at += k;
          r.size -= k;
          if (r.at < chunk.length) continue;
          r.chunks[r.first++] = null;
          r.at = 0;
          // not a shift() of each: that moves all of a long list
          if (r.first === r.chunks.length || r.first === 1024) r.first -= r.chunks.splice(0, r.first).length;
        }
        if (r.size < r.window) release(r);
        return n;
      },
      close(id) {
        const r = open.get(id);
        if (!r) return 0;
        forget(r);
        return 1;
      },
      networkOff() {
        for (const r of [...open.values()]) fail(r, 'off', 'network access is off');
      },
      shutdown() {
        for (const r of [...open.values()]) forget(r);
      },
    };
  }

  return { create };
})();

if (typeof Module === 'object') {
  Module['webFetch'] = PharoWebFetch.create({
    signal: index => Module['pharoHost'].signal(index),
    network: () => Module['pharoHost'].network,
    fetch: typeof fetch === 'function' ? (url, init) => fetch(url, init) : undefined,
    isBrowser: !(typeof process === 'object' && process.versions && process.versions.node),
  });
  Module['pharoHost'].add(Module['webFetch']);
}
