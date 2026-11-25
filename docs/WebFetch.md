# WebFetch: fetch for Smalltalk on WebAssembly

The WebFetchPlugin lets the code of an image make HTTP and HTTPS requests with the `fetch` of the host: of the browser in the Console, the Notebook and the world (one code path, `vm-worker.js` and `vm-driver.js`), and of node in the node VM.
The Smalltalk side is the repository `web-fetch.st`: the package `WebFetch`, which waits for a request as Smalltalk code does, on a semaphore, so that the VM and every other process go on meanwhile, and `WebFetch-Zinc`, an adapter that gives `ZnClient` the same fetch when it is installed. Its README is the guide of the API.
Native VMs have no such plugin: there `WebFetch availability` answers `#unavailable`, and every request signals `WebFetchUnavailable`.

Below it is a mechanism that other services can use: an event of the host, between two slices of the VM, signals any external semaphore of the image.

Files:

- `packaging/emscripten/host/host-events.js`, the events, and `packaging/emscripten/host/web-fetch.js`, the requests: `--pre-js` of both VMs;
- `src/emscripten/plugins/WebFetchPlugin.c`, the six primitives, and `primitiveMayWait` in `WebHostPlugin.c`;
- `vm_signal_semaphore` in `src/emscripten/emscriptenMain.c`, and the sleep of an idle VM in `src/common/heartbeat.c`;
- `packaging/emscripten/node/node-pump.js` and `packaging/emscripten/web/vm-driver.js`, the hosts;
- `packaging/emscripten/web/network-setting.js`, the setting of the network, with the pages, `vm-worker.js`, `nb-kernel.js` and `src/emscripten/gitSupport.c`;
- `tests/wasm`: see Tests.

## Events of the host

`Module.pharoHost` (`host-events.js`):

- `signal(index)` signals the external semaphore `index` (`Smalltalk registerExternalObject:`) and wakes the VM: a sleep of the idle VM is cut short. It answers false, and does nothing, for an index that the table does not hold and once the VM is gone;
- `add({networkOff(), shutdown()})` adds a service;
- `network` is whether the image may use the network, and `setNetwork(on)` changes it: off calls `networkOff()` of every service;
- `shutdown()`: the VM exited or crashed. It signals nothing any more, and `shutdown()` of every service is called, once. A service that throws is reported on stderr and stops no other.

A service keeps the index in a record of its own, forgets the record synchronously, in the primitive with which the image closes it, and signals only for a record that it still has; the image unregisters the semaphore after that.
So a late event never reaches the index once another semaphore has it, and no table in C, no session in an id and no queue of signals are needed.

`Module.hostWake()` is the host's: `vm-driver.js` runs a slice soon, unless the output is paused (`resumeOutput()` then delivers the signal), and `node-pump.js` one at once.
Both call `shutdown()` when the VM ends, however it ends: node exits with the status of the VM although requests are open.

`vm_signal_semaphore(index)` answers 0 before the VM started, after it exited, and for an index outside the table; else it counts a request, which the next event check delivers, so it may be called inside a slice too.
An idle VM does not sleep when such a request is pending (`isPendingSemaphores()` in `ioRelinquishProcessorForMicroseconds`): the longjmp of the sleep would skip the delivery until the next wakeup. Without that 20 signals reached a sleeping VM in a median of up to 19 ms (32 ms at most), with it in 0.4 ms.

## The requests

`Module.webFetch` is made by `PharoWebFetch.create({signal, network, fetch, isBrowser})` (`web-fetch.js`).
A request has an id, a positive integer of 31 bits from a counter that starts at random, and at most 64 are open: the 65th start answers 0.

| State | | Next |
|---|---|---|
| 1 PENDING | no answer yet | 2, 4 |
| 2 HEAD | status and headers; `size` bytes of the body are ready, and it goes on | 3, 4 |
| 3 DONE | all of the body is here, `size` bytes unread | |
| 4 FAILED | the record is the error's; the bytes ready are dropped | |

It signals its semaphore when it enters 2, 3 and 4, when `size` goes from 0 to more, and when `size` reaches the window, until it is closed; never inside a primitive.
The body is read only while less than `window` bytes are ready (the last chunk may exceed it). A whole body is started with a window of one byte more than its maximum, so that a body of exactly the maximum is seen to end, and one that is larger is known by `size`; a stream has a window of 1 MiB.
`timeoutMs`, when it is not 0, is the time from the start to DONE.

The records are UTF-8 with a LF between the lines; a CR, a LF or a NUL in a part becomes a space:

- request: `METHOD`, `URL`, then `name: value` lines;
- answer: `status`, status text (empty over HTTP/2), final URL, type, `1` when redirected or `0`, then `name: value` lines;
- error: kind, message. `request`: it is not valid; `network`: no answer, with the cause in node; `body`: the answer broke off; `timeout`; `redirect`: a redirect that a browser does not show; `off`: the network was turned off; `aborted`: aborted by the host.

A request fails with the kind `request` when its URL is not an absolute `http:` or `https:` one, has a user name or a password, or when `new Request()` refuses it.
These headers of the image are dropped: accept-charset, accept-encoding, access-control-request-headers, access-control-request-method, connection, content-length, cookie, cookie2, date, dnt, expect, host, keep-alive, origin, referer, set-cookie, te, trailer, transfer-encoding, upgrade, via, `proxy-*` and `sec-*`, and in a browser user-agent.
The request is always made with `mode: 'cors'`, `credentials: 'omit'`, `referrerPolicy: 'no-referrer'` and `cache: 'default'`, and `redirect` is `'manual'` for flags 1, else `'follow'`.

The methods are synchronous, never throw and never call the VM; the comment at the top of the file says them all.

## The primitives

Module `WebFetchPlugin`, built into both VMs. None of them waits, so they work inside an FFI callback too.

| Primitive | Arguments | Answers | Fails with |
|---|---|---|---|
| primitiveFetchAvailability | none | 0 no fetch here, 1 ready, 2 the network is off | never |
| primitiveFetchStart | semaphoreIndex, head, body, window, timeoutMs, flags | the id | `bad argument`: an index outside the table, a head that is no ByteArray of 1 to 1048576 bytes, a body that is neither nil nor a ByteArray of at most 2^31-1 bytes, a window not in 1..2^31-1, a timeout not in 0..2^31-1, flags other than 0 and 1; `unsupported operation`: availability is not 1; `resource limit exceeded`: 64 are open; `insufficient C memory`: JavaScript could not copy the body |
| primitiveFetchState | id | state + 8 * the bytes ready | `bad argument`: no SmallInteger; `not found`: no such request |
| primitiveFetchHead | id | the record, a ByteArray, or nil in state 1 | the same; `insufficient object memory` |
| primitiveFetchRead | id, aByteArray, start, count | the bytes copied, 0 to count | the same; `bad argument`: no ByteArray; `no modification`: it is read-only; `bad index`: the range is not inside |
| primitiveFetchClose | id | true when it was open, else false | `bad argument` |

A CompiledMethod, which is a kind of ByteArray in Pharo, counts as none.
`primitiveMayWait` of the WebHostPlugin answers false inside an FFI callback, where a wait would never end (the VM cannot return to its host there): the package signals `WebFetchInCallback` instead of waiting, in the process that evaluates the callback. Another process may wait: the event comes after the callback.

## Memory

The body stays in the JavaScript heap until it is read, and `primitiveFetchRead` copies it straight into the object of the caller; none of it is in the C heap, which never gives its memory back (see Limitations of [WebAssembly.md](WebAssembly.md)).
A whole body is read once, when it is DONE: one copy in the image. A body of 60 MB took old space from 95 MB to 162 MB at the peak, and to 146 MB after a garbage collection that kept it.
A request body is copied once, at the start. There is no cap over all requests: the windows are the caps.

## The setting of the network

The Settings dialog of both pages has "Smalltalk code may use the network", on by default.
Off is kept in localStorage, `pharo-wasm.network` = `off` (absent is on), for every page of the site, and other tabs follow at once; without localStorage it holds for the page. It is never taken from the URL.
It governs the three ways out of the image:

- fetch: `availability` answers 2, a start fails, and the open requests fail with the kind `off`;
- git (`gitSupport.c`): "git http: network access is off in Settings", before any request, with no advice about CORS;
- the sockets: the WebSocket that Emscripten makes for one cannot be made (`EHOSTUNREACH`), and those open are closed (`isConnected` may answer true until that closing is over, but nothing is sent).

A change holds at once, also while the VM boots (`init.network`, the message `network` and `ready.network` of `vm-worker.js`). The fetches of the worker itself, of the files of the site, are not concerned.

In the node VM `PHARO_WASM_NETWORK=0` in the environment turns fetch and git off. It is a convenience, not a boundary: the sockets, the FFI and the files of the host are there.

## Security

- A request never carries credentials (cookies, HTTP authentication of the browser, client certificates) nor a referrer, and is only of http: and https:; nothing of the image is evaluated as JavaScript.
- With the network on, the code of an image may read every URL that answers with CORS, and send data to any URL, also to one that answers without CORS: a request that the browser refuses to show has been made. An image that someone else made is code that runs with that right; turn the network off first.
- The worker must be loaded from the network, as the pages do. `connect-src 'self'` in the `<meta>` of the Console is for the page and not for a worker that came by itself: a blob: worker would inherit it, and a server that sends a Content-Security-Policy header of its own with `vm-worker.js` blocks the requests that its `connect-src` does not allow (run in Chromium 156). Nothing else of the pages relies on it.

## In a browser

- Another origin answers only with CORS: `api.github.com` and `raw.githubusercontent.com` do, the zip archives of `codeload.github.com` do not. A refusal, a name that is not there, a TLS error and a network that is down are all the same error, without a reason (the console of the browser has it).
- `Authorization`, a `Content-Type` such as `application/json` and headers of one's own cost a preflight, an OPTIONS request before each.
- Redirects are followed by the browser: the last answer is all that is seen, and not to follow them is an error of the kind `redirect`. (Node gives the 3xx answer.)
- Over HTTP/1.1 a browser opens six connections to a host, so a seventh request waits for one of them; and Chromium lets simultaneous GETs of one URL wait for one another (its cache).
- A wait in the UI process of the world freezes it until the request ends, as a socket does natively. Stop opens a debugger on that process and the world draws again; the request stays open until the debugger is closed.

## Tests

- `tests/wasm/web-fetch.test.mjs`: `host-events.js` and `web-fetch.js` in node with a fake fetch, and `node-pump.js` with a fake module, 24 checks.
- Lane 81-webfetch (`tests/wasm/lanes/81-webfetch.sh`, with `tests/wasm/st/webfetch-lane.st`): that test; the primitives and every failure code above in the node VM and in the web VM (80 checks each), an FFI callback, the memory, an exit and a trap with open requests, the network off, a node without fetch, a VM without the plugin. With `WEB_FETCH_DIR`, a checkout of `web-fetch.st`, it loads the packages with Metacello and runs their tests, also the live ones, in the node VM, there with the network off, and in the web VM, requests while a real FFI callback runs (the node VM), and an image saved while processes wait; without it that part is a skipped check.
- `tests/wasm/vm-harness.js` (lane 60), cases 28 to 30: how soon a signal wakes a process, the pause of the output, the services at an exit and at a crash. `tests/wasm/worker-harness.js` (lane 70), case 31: the network of the worker.
- `tests/wasm/webfetch.spec.mjs` (`make wasm-check-browser`): the pages in a browser, 17 checks, 19 with `WEB_FETCH_DIR`, where both suites run in the Console too.
- `tests/wasm/lib/fetch-http.mjs` is the server of all of them; `node tests/wasm/lib/fetch-http.mjs` runs it and prints its URL, for `WEB_FETCH_SERVER` of the live tests.

Only Chromium was run; Firefox and Safari were not.

## Left out

The DOM, JavaScript code, WebSocket and EventSource, OPFS and WebCrypto for the image: they can be services of `pharoHost`. Credentials and cookies; streamed request bodies; the hops of a redirect; another cache mode; a CORS proxy for HTTP (the git proxy forwards only git).
A ZnServer in the node VM: `listen` ends it without the npm module `ws`, as before.
