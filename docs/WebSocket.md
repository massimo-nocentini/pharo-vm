# WebSocket: WebSocket clients for Smalltalk on WebAssembly

The WebSocketPlugin lets the code of an image open WebSocket client connections with the `WebSocket` of the host: of the worker in the Console, the Notebook and the world, and of node in the node VM. It sends and receives whole messages, text and binary, and closes them, while the VM and the other processes go on: a process waits on a semaphore, which an event of the host signals (`Module.pharoHost`, see [WebFetch.md](WebFetch.md), Events of the host).
The Smalltalk side is the package `WebFetch-WebSocket` of the repository `web-fetch.st` (class `WebFetchWebSocket`, group `WebSocket` of `BaselineOfWebFetch`), with `WebFetch-WebSocket-Tests`. Its README is the guide of the API.
Native VMs have no such plugin: there `WebFetchWebSocket availability` answers `#unavailable`, and `connect:` signals `WebFetchUnavailable`; so does an image on a VM of before this plugin, where fetch still works.

It follows the fetch service of [WebFetch.md](WebFetch.md) in every point where it can; where this file says nothing, the rule of fetch holds.
Emscripten's sockets, which carry the TCP bytes of the SocketPlugin over WebSockets of their own to a websockify-style proxy, are another thing and are not touched.

Files:

- `packaging/emscripten/host/web-socket.js`, the service, `--pre-js` of both VMs after `web-fetch.js`; its header comment is the contract of its methods, states, signals and timers;
- `src/emscripten/plugins/WebSocketPlugin.c`, the ten primitives;
- `packaging/emscripten/node/node-pump.js`, the exit guard, and `packaging/emscripten/web/vm-worker.js`, the network of the worker and the hostname of the site;
- `tests/wasm`: see Tests.

## The service

`Module.webSocket` is made by `PharoWebSocket.create({signal(index), network(), WebSocket(), isBrowser, siteHostname, setTimeout, clearTimeout})`, and added to `pharoHost`.
`WebSocket()` answers the constructor at each open (`globalThis.WebSocket`, which is the guard of `vm-worker.js` in a worker); `isBrowser` and `siteHostname` (`self.location.hostname` in the worker, through `Module.siteHostname`) are for the refusal of the hosts of the site's domain (Security); every timer is made with the `setTimeout` given and, in node, `unref()`'d, so that no timer of the service keeps node alive.
The file declares no binding named `WebSocket`: Emscripten's sockets take the constructor by that unqualified name in the scope of the factory, where the `--pre-js` files are.

A socket has an id, a positive integer of 31 bits from a counter that starts at random, and three indexes of external semaphores. At most 32 are open, closing ones counted (see Lingering).

| State | | Next |
|---|---|---|
| 1 CONNECTING | the handshake goes on | 2, 3, 5 |
| 2 OPEN | messages come and go | 3, 4, 5 |
| 3 CLOSING | the image asked to close, or the server sent its close frame (seen through `readyState`: no event comes for it) | 4, 5 |
| 4 CLOSED | the `close` event came, from either side, clean or not, or the close timer expired; the messages queued stay readable | |
| 5 FAILED | the info record is the error's; the messages queued stay readable | |

The events and the timers move it so, and nothing else does:

| In state | `open` | `message` | `error` | `close` event | handshake timer | close timer | `networkOff()` |
|---|---|---|---|---|---|---|---|
| 1 | 2 | | 5 `network` | 5 `network` | 5 `timeout`, closed | | 5 `off` |
| 2 | | queued, or 5 `tooLarge` / `overflow` | ignored (a `close` follows) | 4 | | | 5 `off` |
| 3 | ignored | the same | ignored | 4; 1006 `closed before it was open`, not clean, when the close began in state 1 | | 4: 1006 `closing handshake timed out`, not clean | 5 `off` |
| 4, 5 | ignored | ignored | ignored | ignored | | | |

A close in the slice of the open, before the `WebSocket` is made, makes none: state 4 the same way, from a microtask. Entering 4 or 5 clears the timers of the socket.
A socket fails with `tooLarge` and is closed with 4009 `message too large`, with `overflow` and closed with 4008 `too many unread messages`, with `off` and closed with 1000 `network access is off`. The codes 4008 and 4009 are of the private range, since a client may not send 1008 nor 1009; they tell the server why.

**Three semaphores per socket**, `state`, `read` and `write`, as the native `Socket` has, because one signal wakes one waiter and a socket has a reader and a writer that wait for different things:

| Event | state | read | write |
|---|---|---|---|
| enters 2, 4 or 5 | yes | yes | yes |
| the queue goes from empty to one message | | yes | |
| `bufferedAmount` after a send crosses 1 MiB (`LOW_WATER`) downward or reaches 0, or the ticker sees `readyState` 2 or more while it drains | | | yes |

Entering 3 signals nothing: from the image it happens inside `close()`, from the server it is no event; the close timer makes it 4 within 5 s. The rule of `host-events.js` holds per socket: after `release` none of its three indexes is signalled.

**Timers.** The handshake timer of a socket (`timeoutMs`, 0 none), cleared on 2; its close timer, `CLOSE_TIMEOUT` (5 s), armed by `close()` and when `state()` or the ticker first sees `readyState` 2 in state 2; and one ticker for the service, a chain of timeouts that runs while a socket drains (`bufferedAmount` above 0 after a send) or is open: every 16 ms while a draining socket made progress in the last second, every 250 ms while none did (a deaf peer), every 500 ms while it only watches the open ones for `readyState` 2. A browser counts the sends discarded while CLOSING in `bufferedAmount`, which then never reaches 0, so a socket stops draining, and `write` is signalled, when `readyState` is 2 or more too.

**Lingering.** A socket that the image forgets (`release`, `shutdown`) whose `WebSocket` has had no `close` event yet lingers until that event, and counts towards the 32: a loop of opens and releases to a server that never answers a close is bounded. In node, where such a socket is never closed, its slot stays taken until the VM ends.
A socket that ends before its handshake does (the handshake timer, a close, a release, the network turned off) does not linger: node 24 and 25 fire the `error` and `close` events of a `WebSocket` still connecting inside its `close()`, and leave `readyState` at 2 after them, so the service handles the events that come inside a `close()` of its own after a microtask (nothing signals inside a method), and never counts a `WebSocket` whose `close` event came. Node drops that TCP connection at once; its pool may open a fresh one to the same server, which ends with its keep-alive timeout of 4 s (measured).

**Memory across sockets.** At most 256 MiB (`MAX_QUEUED_TOTAL`) are queued in all the sockets of a VM: a message that would cross it fails its socket with `overflow`, `the queues of all sockets are full`. So `maxMessage` and `maxQueued` are at most 256 MiB too: a larger one could never be reached.

**A message.** In `onmessage`, a string longer than `maxMessage` fails the socket with `tooLarge` before it is encoded (UTF-8 has never fewer bytes than UTF-16 units); else it is encoded with `TextEncoder`, its size checked again, and the string dropped. An `ArrayBuffer` is checked by its `byteLength` (`binaryType` is `arraybuffer`). The message that fails a socket is not queued; the ones before it are.

The methods are synchronous, never throw and never call the VM; nothing signals inside them. The header comment of `web-socket.js` says them all; in short:

| Method | Answers |
|---|---|
| `availability()` | 0 no `WebSocket` here, 1 ready, 2 the network is off |
| `open(semState, semRead, semWrite, record, maxMessage, maxQueued, timeoutMs)` | the id, or 0 when availability is not 1, after `shutdown()`, or when 32 are open or lingering; the record is checked, and the `WebSocket` made, after a microtask: a record that fails the checks makes the state 5 `request` there |
| `state(id)` | state + 8 * the number of messages queued; -1 unknown id |
| `infoSize(id)`, `info(id, dest)` | the size of the info record (-1 in state 1), and a copy of it |
| `peek(id)` | for the C side: -1 when none is queued, else 2 * its size, + 1 for text |
| `receive(id, dest)` | copies the next message into `dest` and drops it; its size, or -1 |
| `drop(id)` | drops the next message without copying it; its size, or -1; a socket that is not over then fails with `tooLarge`, `a message of <n> bytes is larger than the image can hold`, closed with 4009 |
| `send(id, bytes, isText)` | `bufferedAmount` after it; -1 unknown id; -2 `readyState` is not 1, or the socket is over; -3 the buffer is full, nothing sent; -4 not UTF-8 (text) or over 64 MiB; -5 the copy failed. Binary is copied from the heap, text decoded from it (`fatal`, a leading BOM kept) |
| `buffered(id)` | `bufferedAmount`, -1 unknown id |
| `close(id, code, reason)` | 1 when it began the closing handshake (also in state 1); 0 in 3, 4 or 5; -1 unknown id; -2 a reason that is not UTF-8, or a code or a reason that the `WebSocket` refused |
| `release(id)` | 1 when the id was known, else 0; forgets the socket at once; its `WebSocket`, unless closed, is closed with 1000 `released` (unless closing) and lingers |
| `lingering()` | the forgotten `WebSocket`s without their `close` event |
| `networkOff()` | every socket in 1, 2 or 3 fails with `off`, its queue kept |
| `shutdown()` | every socket forgotten, closed with 1000 `the VM ended` (unless closing) and lingering; every timer cleared; nothing signals; `open` answers 0 after it |

The records are UTF-8 with a LF between the lines; in those of the host a CR, a LF or a NUL in a part (a reason of the server) becomes a space:

- open (the image's): URL, then one offered subprotocol per line. A URL that is not an absolute `ws:` or `wss:` one, or has a user name, a password, a fragment, a CR or a NUL, in a browser one of a host of the site's domain, and a subprotocol that is no token of RFC 7230 or is offered twice (ignoring case), are refused with the kind `request`, never changed;
- info, in 2 and 3: chosen protocol (empty for none), extensions (empty for none), URL; in 4 the same, then the close code, the reason, `1` when clean or `0`;
- error, in 5: kind, message, then chosen protocol, extensions, URL (empty when it never opened). The kinds are `request`, `network`, `timeout`, `off`, `tooLarge` and `overflow`.

A handshake that fails is `network`, `the handshake failed (no answer, a refusal, or a status other than 101)`, followed by the message and the code of the cause of the error event when it has them (node 24 and 25 give neither, and a browser's error event is a plain `Event`). An exception of the constructor is `request` with its name and message; a `SecurityError` is `off` while the network is off, else `request`.

## The primitives

Module `WebSocketPlugin`, built into both VMs (`listBuiltinModules` says `WebSocketPlugin (i)`), written by hand in the form Slang generates, as the WebFetchPlugin is. None of them waits, so all work inside an FFI callback. An id is a SmallInteger; 0, a negative one and one of 2^31 or more are `not found`.

| Primitive | Arguments | Answers | Fails with |
|---|---|---|---|
| primitiveWebSocketAvailability | none | 0 none here, 1 ready, 2 the network is off | never |
| primitiveWebSocketOpen | stateIndex, readIndex, writeIndex, record, maxMessage, maxQueued, timeoutMs | the id | `bad argument`: an index outside the table, two indexes alike, a record that is no ByteArray of 1 to 65536 bytes, a maxMessage or a maxQueued not in 1..2^28 (256 MiB, `MAX_QUEUED_TOTAL`), a timeout not in 0..2^31-1; `unsupported operation`: availability is not 1; `resource limit exceeded`: 32 are open or lingering; `insufficient C memory`: JavaScript could not copy the record |
| primitiveWebSocketState | id | state + 8 * the messages queued | `bad argument`: no SmallInteger; `not found` |
| primitiveWebSocketInfo | id | the info record, a ByteArray, or nil in state 1 | the same; `insufficient object memory` |
| primitiveWebSocketReceive | id | the next message as `{isText. bytes}` (a Boolean and a ByteArray), which leaves the queue; nil when none is queued | the same as State; `insufficient object memory`: the message stays queued |
| primitiveWebSocketDrop | id | the size of the message dropped (the socket fails with `tooLarge` unless it is over), or nil | the same as State |
| primitiveWebSocketSend | id, aByteArray, isText | `bufferedAmount` after | `bad argument`: no ByteArray, isText no Boolean, over 64 MiB, not UTF-8 for text; `not found`; `inappropriate operation`: `readyState` is not OPEN, or the socket is over; `resource limit exceeded`: the buffer is full, wait on `write`; `insufficient C memory` |
| primitiveWebSocketBuffered | id | `bufferedAmount` | the same as State |
| primitiveWebSocketClose | id, code, reason | true when it began the closing handshake, false when it was closing or over | `bad argument`: a code other than 1000 and 3000..4999 (what `WebSocket.close` takes), a reason that is neither nil nor a ByteArray of at most 123 bytes of UTF-8; `not found` |
| primitiveWebSocketRelease | id | true when it was known, else false | `bad argument` |

A CompiledMethod counts as no ByteArray. Receive asks `peek` for the size and the type, allocates the ByteArray and the Array (the first kept through `pushRemappableOop`), then copies once with `receive`: no JavaScript runs between those calls, so the message is the same one, and no slice can end between learning of a message and taking it.
A plugin of its own, not more primitives of the WebFetchPlugin: a host may have fetch and no `WebSocket` (node 20 and 21 without a flag), and an image must tell the two apart.

## The package

`WebFetchWebSocketHost` has one method per primitive and one failure method per primitive (its class comment maps every failure code); `WebFetchWebSocketCloser` releases a socket that was dropped, and unregisters its three semaphores; `WebFetchWebSocket` is the API.
No Mutex is held while a process waits: one `lock` per socket guards only the primitive calls and what the socket records. A wait for the open or the end is on `state`, `next` on `read`, a send and `flushTimeout:` on `write`; a waiter that waited and got what it waited for signals its semaphore once more (the baton), so that a second reader or writer looks again.
When the image first sees state 4 or 5 it settles the socket: it reads every queued message into the image, reads the info record, releases the host record and unregisters the semaphores, and wakes every waiter. From then on the socket needs no host: its slot is free at once, and what it holds survives an image save (`shutDown:` settles the sockets that are over before a save). After a resume the sockets that were not settled end with `WebFetchSessionEnded`.
`insufficient object memory` from Receive makes the package collect garbage and grow the memory by `maximumMessageSize` and 1 MiB, then try once more, then Drop: the socket fails with `WebFetchTooLarge`, and the messages before that one stay readable. (A garbage collection alone does not make room for a message of 60 MB in the stock image: measured.)

## Limits

| Limit | Value | Where | When exceeded |
|---|---|---|---|
| sockets per VM | 32, lingering ones counted | `MAX_OPEN` | `resource limit exceeded`, `WebFetchTooManyOpen` |
| open record | 65536 bytes | C | `bad argument`, `WebFetchBadRequest` |
| subprotocols | tokens, no two alike ignoring case, at most 32 | Smalltalk, then `web-socket.js` | `WebFetchBadRequest` |
| one message received | `maximumMessageSize`, 16 MiB by default, 1..2^28 (256 MiB) | per socket | the socket fails with `tooLarge`, closed with 4009; the queue stays readable |
| bytes queued unread | `maximumQueuedBytes`, 16 MiB by default, 1..2^28 (256 MiB), and 65536 messages | per socket | the socket fails with `overflow`, closed with 4008; the queue stays readable. A message of at most `maximumMessageSize` always enters an empty queue, while the queues of all sockets have room for it |
| bytes queued in all sockets | 256 MiB, which bounds the two limits above | `MAX_QUEUED_TOTAL` | the socket whose message crosses it fails with `overflow`, `the queues of all sockets are full` |
| one message sent | 64 MiB | `MAX_SEND`, checked in Smalltalk first | `WebFetchTooLarge`, the socket stays open |
| send buffer | 16 MiB (`HIGH_WATER`) | `web-socket.js` | a send is refused while `bufferedAmount` is above 0 and would exceed 16 MiB with it; one message of up to 64 MiB goes on an empty buffer; `write` is signalled at 1 MiB and at 0 |
| close code, reason | 1000 or 3000..4999; at most 123 bytes of UTF-8 | Smalltalk, C and JavaScript | `bad argument`, `WebFetchBadRequest` |
| handshake | `timeout`, 30 s by default; none with nil | per socket | the socket fails with `timeout` |
| closing handshake | 5 s (`CLOSE_TIMEOUT`) | `web-socket.js` | state 4, 1006, `closing handshake timed out`, not clean; in node also when the server answered the close but ends TCP later (In node) |

Measured by `tests/wasm/websocket.spec.mjs` in Chromium 156 (headless, through Playwright 1.64), as it prints them:

- against `/ws/slow-read`, which does not read for 5 s, 16 sends of 1 MiB went and the 17th was refused with `bufferedAmount` at 16777216: the send buffer of the service is the limit, not one of Chromium's; `write` was signalled once the server read;
- against `/ws/deaf`, which never answers a close, the close timer ended the socket after 5.0 s, and Chromium ended TCP 60.1 s after the close: its own closing timeout is 60 s, and the socket lingers that long;
- three handshakes to one port opened in under 30 ms while a handshake to another port of the same address never ended, and a second handshake to the port of that pending one had not reached the server 1.5 s later: Chromium makes one handshake at a time per address and port, not per address;
- a text message that begins with U+FEFF arrives without it, as in node;
- 1500 messages of 1 KiB at once, which the Console prints one by one (1.5 MB of output, more than the output that the worker lets wait): all read, the socket closed cleanly, in about 0.5 s;
- the peak memory of the worker for a message of 60 MB was not measured: `performance.measureUserAgentSpecificMemory` needs a cross-origin isolated page, which the site is not.

Only Chromium was run; Firefox and Safari were not.

## Memory

A message stays in the JavaScript heap (a Uint8Array; a text message is encoded to UTF-8 when it arrives, and its string dropped) until `primitiveWebSocketReceive` copies it into a new object of the image, once; none of it is in the C heap. A message of 60 MB costs one copy in old space (lane 84).
A send is copied once out of the heap for binary, and decoded straight from the heap for text, before `ws.send`.
The WebSocket API has no back-pressure on receiving: the engine delivers messages whether or not they are read, and holds a whole message before `message` fires. The limits bound what is queued, not what the engine buffered.
In the image, a text message with a character above 255 becomes a WideString of 4 bytes per character: a message of 16 MiB may take 64 MiB of old space. `nextRaw` answers the UTF-8 bytes instead.

## The setting of the network

The WebSockets of the image are the fourth way out of it, with fetch, git and Emscripten's sockets, and the setting of the pages (`docs/WebFetch.md`, The setting of the network) governs them:

- off, `availability` answers 2, an open fails with `unsupported operation` (`WebFetchNetworkOff`), and the sockets that are not over fail with the kind `off`, their queues kept, and are closed with 1000 `network access is off`;
- in the worker, `setNetwork` of `vm-worker.js` calls `pharoHost.setNetwork` first, then closes the WebSockets of its guard (Emscripten's, and those of the service, whose close then does nothing). The guard stays: a `WebSocket` made while the network is off throws a `SecurityError`, which the service maps to `off`;
- in node, `PHARO_WASM_NETWORK=0` turns WebSockets off too.

## Security

- The handshake of the worker carries the cookies that the browser holds for the target host, by its rules: from the site to another site only `SameSite=None` cookies, which are `Secure`, while third-party cookies are allowed (measured in Chromium: such a cookie of `localhost` went with a handshake from a page of `127.0.0.1`, and a `Lax` or `Strict` one did not); to a host of the same site, every cookie that the browser holds for it, `Lax` and `Strict` ones too, among them the cookies of the site set with a `Domain` attribute. It may carry the HTTP authentication and the client certificates that the browser has for that host too. This is the exception to "a request never carries credentials" of [WebFetch.md](WebFetch.md).
- The server sees the `Origin` of the site (measured): a server that checks `Origin` treats the image as the site. A server that authenticates WebSockets by cookies and does not check `Origin` is open to the code of any image (cross-site WebSocket hijacking), as it is to any page.
- In a browser the service refuses a URL whose hostname is of the site's domain, whatever its port, since cookies ignore ports: the site's own hostname, a parent or a subdomain of it, or a hostname with the same last two labels (`api.example.org` and `example.org` from a site at `www.example.org`); an IP address only when it is the site's. Kind `request`, `a host of the domain of this site is refused: its cookies would be sent`. A cookie set with a `Domain` attribute goes to every host of its registrable domain, which has two labels or more, and those hosts are of the same site, so its `Lax` and `Strict` cookies go there too; without the list of public suffixes the last two labels stand for the registrable domain, which refuses more than needed under a public suffix of two labels (`co.uk`, `github.io`). It keeps the cookies of the site, where the risk is highest, out of the image's handshakes; it also refuses a WebSocket server of the same host or domain as the pages.
- The image cannot set headers (no `Authorization`): tokens go in the URL or in a subprotocol, and a URL may end up in the logs of servers.
- A URL must be an absolute `ws:` or `wss:` one, with no user name, no password and no fragment; that is stricter than the engines, which take `http:` and `https:` and change them.
- From an https: page a browser allows only `wss:` (mixed content), but to localhost: a `ws:` URL then throws a `SecurityError`, which is `request`, `not allowed from this page (an https: page reaches only wss:)`. The spec serves its pages over http:, so this is checked by the unit test of the service only.
- With the network on, the code of an image may talk to any WebSocket server; turn the network off before running an image that someone else made.

## In a browser

- No CORS: any server that answers the handshake is reached, and sees the `Origin` of the site. The `connect-src` of a CSP of the worker applies; the `<meta>` CSP of the Console is the page's and not the worker's (the spec sees no violation).
- A refusal, a name that is not there, a TLS error and a handshake that fails are all the same error, without a reason (the console of the browser has it); a lost connection is a close with 1006, not clean.
- Chromium makes one handshake at a time per address and port (RFC 6455, 4.1; measured, see Limits): sockets to one server open one after the other, and the timeout of a socket counts the time it waits behind the others.
- The browser answers the pings of the server by itself; the image cannot send one. Chromium offers `permessage-deflate; client_max_window_bits`; `extensions` says whether the server took it.
- The worker pauses the VM while more than 1 MiB of output is not acknowledged (docs/WebAssembly.md, The heartbeat), and the reading of WebSockets stops with it while their queues grow: a program that prints every message of a flood slows itself, and a socket may end in `overflow`. So does a long FFI callback.
- `connect:`, `next` and a send that waits, in the UI process of the world, freeze it, as a socket does natively; Stop opens a debugger on that process, the world draws again, and the socket stays open. Tools use `start` and `whenMessage:whenClosed:` with `UIManager default defer:`.

## In node

Node 24 and 25 send no `Origin`, offer `permessage-deflate`, give no reason for a failed handshake (an empty message and no cause), take no proxy from the environment, and strip a leading BOM of a text message received.
After the closing handshake node waits for the server to end TCP, and fires `close` only then; nothing tells that the server's close frame came (no event, `readyState` is 2 already, its socket and its parser are private). So the close timer cannot tell a server that answered the close from one that never does: a server that answers but ends TCP more than 5 s later ends the socket with 1006 `closing handshake timed out`, not clean, and its code and reason are not known. Measured: `wss://echo.websocket.org` answers a close at once and ends TCP about 5.3 s later, so its sockets in the node VM usually end that way.
A WebSocket of node keeps the event loop alive until the server ends TCP, also after a close that the server never answers, and Emscripten's exit lets the loop drain. So `node-pump.js`, after `pharoHost.shutdown()`, arms an `unref()`'d timer of 1500 ms when sockets still linger, which flushes stdout and stderr and exits with the status of the VM; a run without a lingering socket arms nothing (`PHARO_WASM_STATS=1` prints `exitGuard=0` or `1`).

## Tests

- `tests/wasm/ws-server.test.mjs`: the WebSocket server of the tests, `tests/wasm/lib/ws-server.mjs`, which `lib/fetch-http.mjs` attaches to its HTTP server, with a second listener that never answers (`/ws/hang`; the comment at the top of `ws-server.mjs` says its endpoints), 19 checks; `tests/wasm/web-socket.test.mjs`: the service on a fake `WebSocket` and fake timers, every cell of the transition table, the exit guard of `node-pump.js`, a fake `WebSocket` that closes as node's does while connecting, and node's own `WebSocket` against a server that never answers the handshake, 24.
- Lane 84-websocket (`tests/wasm/lanes/84-websocket.sh`, with `tests/wasm/st/websocket-lane.st`): those two; the plugin is built in; every answer and failure code of the ten primitives in the node VM and in the web VM (134 checks each); lingering sockets; an FFI callback; a message of 60 MB; a node without `WebSocket`; `PHARO_WASM_NETWORK=0`; a VM without the plugin; an exit and a trap with three sockets open, one to a server that never answers; a quiet exit; the network turned off in the web VM. 14 checks. With `WEB_FETCH_DIR`, a checkout of `web-fetch.st`, it loads `WebFetch-WebSocket` and runs its 67 tests, the live ones against the server, in the node VM, there with the network off, and in the web VM; waits in a real FFI callback; and an image saved while processes wait, restarted: 21 checks.
- `tests/wasm/vm-harness.js` (lane 60), case 30, and `tests/wasm/worker-harness.js` (lane 70), cases 31 and 31b: the service at an exit, and the order of the network of the worker.
- `tests/wasm/websocket.spec.mjs` (`make wasm-check-browser`): the Console, the Notebook and the world in Chromium, against the server reached as `localhost`, with `tests/wasm/st/websocket-spec.st`: echo, the subprotocol, the `Origin` and a cookie, the refusal of the site's hostname (an IP address there), the BOM, handshakes behind a pending one, Stop, 1500 messages printed, a process of the Notebook, a morph of the world, Stop in the UI process, and the setting of the network with a WebSocket of the image and a socket of the SocketPlugin open, in each page and in another tab; and the measures above. 15 checks, 16 with `WEB_FETCH_DIR`, where the suite of the package runs in the Console too, with its live tests. It runs in Chromium only: of the browsers that `BROWSERS` names it skips the others, and says so.

## Left out

Servers (no listening socket exists in a browser); ping frames from the image (the API has none); streams of fragments (the API delivers whole messages); `WebSocketStream` (back-pressure on receiving; Chromium only); headers of the image's own; `ZnWebSocket` of Zinc-WebSocket, which frames and handshakes by itself over a socket stream, while the host's WebSocket gives whole messages (a later `WebFetch-WebSocket-Zinc` could give its messages on a `WebFetchWebSocket`).
