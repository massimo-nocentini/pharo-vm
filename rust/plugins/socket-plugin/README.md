# SocketPlugin, in Rust

Replaces the two C halves of the sockets plugin -- the Slang-generated
`SocketPlugin.c` (~2,400 lines of primitive shims) and the hand-written
`SocketPluginImpl.c` (~2,500 lines descended from `sqUnixSocket.c`, 1996) --
with one crate behind the same **60 primitives**: TCP and UDP
create/connect/listen/accept/send/receive/close, socket options, the DNS
resolver with its semaphore, and the 2007 IPv6-capable address API.

## The contract is unchanged

Same module name, same primitive names, same argument shapes, same accessor
depths (checked byte-for-byte against the C's exports: 40 primitives at 0, 20
resolver primitives at -1), same explicit `pop`/`popthenPush` counts as the
generated shims. `initialiseModule`, `shutdownModule` and `moduleUnloaded` are
exported as before. Build the crate and drop `libSocketPlugin.so` where the VM
looks for plugins.

The `aioEnable` / `aioHandle` / `aioDisable` / `aioFini` calls are plain
undefined symbols in the shared object, resolved against the VM core when the
plugin is loaded -- exactly how the C plugin's were. There is no
`ioLoadFunctionFrom` indirection because the C never used one either. Leaving
them undefined is free on ELF and needs one linker flag on Mach-O; see
[macOS](#macos). The
connect/read/write semaphore trio and the resolver semaphore are signalled
through the proxy's `signalSemaphoreWithIndex`, from the same aio handlers at
the same points.

## The socket handle

The image sees a socket as a ByteArray of `sizeof(SQSocket)` bytes: session
ID, socket type, and one pointer. That outer record's layout is frozen -- it
is ABI. What the pointer aims at was `struct privateSocketStruct`; it is now a
Rust type (`sock::PrivateSocket`), heap-allocated on create and freed on
destroy, validated the way the C's `socketValid` validated (non-null private
state, live session, matching session stamp).

**The internal struct's layout is private, with one exception.**
`UnixOSProcessPlugin`'s `socketDescriptorFrom:` reads `*(int *)` through the
private pointer to recover the file descriptor, relying on the descriptor
being the struct's first field (its own comment says it "will break if anyone
ever redefines the data structure"). Nothing else in the tree reaches inside.
So `PrivateSocket` is `repr(C)` with `fd: c_int` first -- a unit test asserts
both that offset and the outer record's layout -- and every other field is
free to change. `UnixOSProcessPlugin`'s CMake link against `SocketPlugin` is
for that header-level coupling only; it resolves no symbols from this library.

## What changed underneath

**The resolver is asynchronous, which on Unix it never was.** The C's
`sqResolverStartNameLookup` blocked the whole VM inside `getaddrinfo` and then
signalled the resolver semaphore on its way out -- its own comment reads
*"we're done before we even started"* -- so every Process in the image stopped
for the length of a DNS round trip, and `ResolverBusy` (2) was a state the Unix
plugin could not reach. The image was written for the other contract the whole
time: `NetNameResolver class >> addressForName:timeout:` takes a mutex, calls
`waitForResolverReadyUntil:`, starts the lookup, then waits on the resolver
semaphore while polling `sqResolverStatus` for `ResolverBusy`, and calls
`primAbortLookup` when it gives up. Pharo 12.0's `NetNameResolver class >>
initialize` even says so in a comment: *"on other platforms, such as Unix, the
resolver is synchronous; a call to, say, the name lookup primitive will block
all image processes until it returns."*

So a start primitive now names a lookup, hands it to a thread and returns.
**No image-side change is needed** -- this is the contract the image already
implements. Three things make it safe, and each is a rule the next asynchronous
plugin will need too:

* **The mutex is never held across the lookup.** `getaddrinfo` runs with
  nothing locked; the resolver state is taken for the microseconds it takes to
  store the answer. Spawning a thread that holds the lock for the whole call
  would only move the freeze from a syscall onto a mutex.
* **Status and error are read lock-free**, out of two atoms (`LOOKUP_BUSY`,
  `LAST_ERROR`) that were moved out of the mutex-protected state, because the
  image polls `sqResolverStatus` in a loop and that poll must never wait for a
  worker.
* **A generation counter decides who may commit.** Every start and every abort
  bumps it under the mutex; a worker that finds the generation moved on drops
  its answer and stays silent. That is what makes `sqResolverAbort` -- an empty
  function in the C -- mean something.

A worker's whole vocabulary is "store a result and increment a counter in the
VM's request table": it reads no oop, allocates nothing in the image and
answers no value. `signalSemaphoreWithIndex` is the VM's designated any-thread
entry point; the primitives that copy bytes out still run on the interpreter
thread, later. Two bounds keep the image from turning a primitive into a
resource: at most 16 workers may be alive at once (over the ceiling a lookup
runs inline, which is exactly what the C did on every call), and
`shutdownModule` answers 0 until every worker thread
has been *joined*, so `Smalltalk vm unloadModule: 'SocketPlugin'` cannot
`dlclose` a library a thread is executing. Joined, not counted: a counter
decremented at the end of a worker's closure reaches zero while the thread is
still running libstd's own epilogue and its TLS destructors, all of which is
code in this same cdylib.

The 2007 `getaddrinfo` API (`primitiveResolverGetAddressInfo` and friends) is
**still synchronous**, and still blocks the VM. Its image-side callers read the
results back without waiting on a semaphore, so making it asynchronous needs an
image change and is out of scope here.

**The resolver holds owned Rust values, not a libc linked list.** The C kept
`getaddrinfo`'s `addrinfo` chain (plus a second, hand-`calloc`ed one for the
AF_UNIX case) in globals and freed them at the top of the next lookup. Those
are now a `Vec<ResolvedAddr>` and an index, filled from
[`dns-lookup`](https://crates.io/crates/dns-lookup), which frees the chain
itself. Gone with them: the `unsafe impl Send` the raw pointers forced, the
`freeaddrinfo`, the `Box::into_raw` pair leaked for the local-socket result,
and the `(*cursor)` dereferences in six accessor functions. `dns-lookup` was
chosen over `std`'s `ToSocketAddrs` because it exposes the raw EAI error
number, which the image reads back through `sqResolverError`; std discards it.

The reverse lookup no longer calls `gethostbyaddr`, which the port had to
hand-declare because it is deprecated, is not thread-safe (it answers a
pointer into static storage) and is IPv4-only. `dns_lookup::lookup_addr` does
the same job over `getnameinfo`. The one observable consequence: a failed
reverse lookup always reports `HOST_NOT_FOUND` now, because `getnameinfo`
reports EAI codes rather than `h_errno` -- and the C's own fallback already
flattened those to `HOST_NOT_FOUND` on any platform without an `h_errno`
accessor.

**Sockets go through `socket2`.** `PrivateSocket.fd` stays the ABI-frozen
`int` that `UnixOSProcessPlugin` reads out of the record, so this plugin
cannot *own* a `socket2::Socket`; a `with_socket` helper borrows the
descriptor as a `SockRef` for the duration of one call, which is exactly the
C's model -- the descriptor is closed by `sqSocketDestroy`, never by a scope
ending. Socket creation, `accept`, `listen`, `shutdown`, `SO_LINGER`,
`SO_REUSEADDR` and `SO_ERROR` are typed calls now.

Two details worth knowing:

* creation and accept use `Socket::new_raw` and `accept_raw`, **not** `new`
  and `accept`, because socket2's non-raw versions set `FD_CLOEXEC` and the
  C's bare `socket()`/`accept()` do not. The difference is observable:
  `UnixOSProcessPlugin` forks and execs, and a descriptor that vanished
  across `exec` would change what child processes inherit. `FD_CLOEXEC` is
  the entire reason, on both platforms. On Apple targets `new` -- `new` only,
  since socket2's `accept` leaves `SO_NOSIGPIPE` to be inherited from the
  listener -- additionally sets `SO_NOSIGPIPE`, but that flag makes no
  difference here: `installErrorHandlers` sets `SIGPIPE` to `SIG_IGN` for the
  whole process (`src/unix/debugUnix.c:150-152`), so `send` on a broken
  connection reports `EPIPE` on Darwin and on Linux either way.
* `with_socket` answers `None` for a negative descriptor rather than
  borrowing one. Several callers do pass `-1`; the C handed it straight to
  `setsockopt`/`getsockopt` and got `EBADF` back, whereas `BorrowedFd` cannot
  represent `-1` and panics. Each caller maps that `None` to whatever the C
  did with the failure.

`libc` remains for what neither covers: `poll(2)` for the VM's own event
loop, `getifaddrs`, `getnameinfo` on a raw image-supplied `sockaddr` (which
may be AF_UNIX, and whose result the C treats as bytes rather than UTF-8),
`gethostname`, the `setsockopt`-by-name table (which needs raw
`(level, optname)` pairs the typed setters do not expose), and the
send/receive calls, whose buffers are raw pointers into image memory either
way.

* **Undefined behaviour removed, failures kept.** Several C shims read an
  argument's bytes with no kind check (`primitiveResolverGetNameInfo`,
  `primitiveSocketAddressGetPort`/`SetPort` and friends) -- a SmallInteger
  argument was a wild pointer dereference. Here those fail cleanly with
  `PrimErrBadArgument`. Likewise, a socket-address payload shorter than the
  `sockaddr` being read from it now fails instead of over-reading, and the
  local-socket `stat` in `getaddrinfo` gets a NUL-terminated path instead of
  unterminated object-memory bytes.
* **`success(false)` became `Err`.** The implementation layer reports failure
  by return value instead of a VM-side flag; the image sees the same failed
  primitive with the same code (`primitiveFail()`'s 1). Where the C stacked
  two failures (a `PrimErrBadArgument` from `socketValueOf:` followed by an
  unguarded implementation call that failed again), this port stops at the
  first, so the *code* can differ (3 where the C ended at 1); the failure
  itself does not.
* **Instantiate-before-pointer in create/accept.** The C took the server's
  record pointer and then instantiated the new handle ByteArray. Spur's
  `instantiateClass:indexableSize:` never runs the GC, so the C was safe in
  practice; the Rust orders the allocation first anyway. Only an
  out-of-memory failure could tell the difference.
* **No logging.** The C's `logTrace`/`logWarn` lines are gone; they were the
  only use of the VM's logging API here.
* **Small leaks tidied where invisible.** `sqResolverLocalAddress`'s error
  path leaked the `getifaddrs` list; freed here. The `connectionStatus` leak
  of an invalidated socket's private state is *kept*, because the C keeps it
  deliberately ("safer not to free").

## Oddities kept on purpose

Faithfulness beats taste; each of these is marked with a comment at the site:

* A "provided" TCP socket (type 65536, systemd socket activation) adopts file
  descriptor 3 unconditionally -- this tree never defines `HAVE_SD_DAEMON`,
  so the C's `sd_listen_fds` stub answers 0 and that is what the C compiles
  to.
* The local-socket path tests `st_mode & S_IFSOCK`, a bitmask intersection
  that regular files also satisfy.
* `accept_from` checks `acceptedSock < 0`, so a server that never accepted
  (field still 0) would adopt fd 0; the image's discipline (accept only after
  the connection semaphore) keeps this unreachable.
* The bind result in the listen path is ignored; errors surface later through
  the accept handler.
* The UDP receive "more flag" is never true.
* Unknown domain codes fall through to `socket()` unchanged (the C switch has
  no default), and buffer-size arguments are accepted and ignored.
* On Linux a `getaddrinfo` failure "succeeds with zero results" and leaves
  `lastError` alone -- glibc does not define `EAI_BADHINTS`, so that is what
  the C's Linux build did. On macOS, whose `<netdb.h>` does define it, the C
  takes the other arm of the same `#if` and reports the error; this port now
  follows each platform's headers rather than applying the Linux arm
  everywhere (see `gai_error_is_fatal`).

## Divergences

Places where the Rust knowingly does *not* do what the C does. Unlike the
oddities above, these are not bugs kept for faithfulness -- they are bugs
fixed, on one platform, because faithfulness there means the primitive does
not work.

* **UDP `sendData:` on Darwin does not send the C's `sendto`.** The C is one
  line -- `sendto(SOCKET(s), buf, bufSize, 0, &SOCKETPEER(s),
  sizeof(SOCKETPEER(s)))`
  (`plugins/SocketPlugin/src/common/SocketPluginImpl.c:1334`) -- and the BSD
  stack refuses it twice over: the oversized `sizeof(union sockaddr_any)`
  length is `EINVAL` on a socket that has no local address yet, and carrying
  a destination at all on a socket `connect(2)` has been called on is
  `EISCONN`. The second is not a corner: `connectTo:port:` calls `connect(2)`
  on UDP sockets, so *every* `sendData:` after one would fail, which is the
  ordinary way an image drives a UDP socket. `sock::udp_send` therefore
  probes with `getpeername` and uses `send(2)` when connected and `sendto`
  with the family's own address length when not; Linux keeps the C's line
  verbatim, where both calls are accepted. A `cfg(macos)` test asserts both
  kernel rules against the kernel, so if a future macOS relaxed either the
  branch can go. The same fix is worth filing against the C at that line.
* **The reverse lookup now signals the resolver semaphore.** The C's
  `sqResolverStartAddrLookup` never called `signalSemaphoreWithIndex` -- only
  the forward lookup did -- and could afford not to, because the answer was
  already in `lastName` by the time the primitive returned and
  `sqResolverStatus` never said `ResolverBusy`, so `NetNameResolver`'s wait
  loop fell straight through. Once the lookup is asynchronous that silence
  costs the image its whole deadline: `waitForResolverNonBusyUntil:` uses
  `waitTimeoutMilliseconds:`, so it would poll to the timeout instead of
  hanging, but a 3 ms lookup would take the caller's `timeout:` seconds.
  Adding a wake-up the image already waits for is the only shape this can
  take.
* **The result accessors refuse while a lookup is in flight.**
  `sqResolverNameLookupResult` and `sqResolverAddrLookupResult(Size)` answer a
  primitive failure rather than the *previous* lookup's `lastAddr` /
  `lastName`. The C could not reach this state; here the old answer is still in
  place until the worker commits, and handing it back would name a host the
  image is no longer asking about, silently. A well-behaved image never sees
  it: `NetNameResolver` reads a result only after the resolver leaves
  `ResolverBusy`.
* **`sqResolverStatus` and `sqResolverError` no longer refuse on a poisoned
  module.** Making them lock-free moved `lastError` out of the mutex, and
  neither word is part of the invariant a panic can tear (that is
  `results`/`cursor`). Nothing is lost: a panic under `poison::lock` sets the
  module flag too, so `run_primitive` fails every primitive of a poisoned
  module before its body runs.
* The sibling primitive is deliberately left alone: `send_udp_to`
  (`sqSockettoHostportSendDataBufCount`, the C's line 1428) still passes an
  explicit destination on every call, and so is exposed to the same `EISCONN`
  rule on Darwin if an image ever calls it on a connected socket. Its address
  length is already `sizeof(struct sockaddr_in)`, so only the second rule
  could bite, and the primitive exists precisely for the unconnected case.

## macOS

Built, tested and green on `aarch64-apple-darwin`. Four things needed a
platform branch, each documented at its site:

* **`S_IFSOCK` is 16-bit here.** `mode_t` is `unsigned int` in glibc and
  `__uint16_t` in Darwin's `<sys/_types.h>`, so the local-socket `stat` test
  that the C writes as one `&` is a type error in Rust on exactly one
  platform. `resolver::s_ifsock` widens with `From`, never `as`, and a
  `const _: () = assert!(...)` per platform pins both the type and the value
  the widening assumes.
* **`getaddrinfo` failures are reported, not swallowed.** See the
  `EAI_BADHINTS` bullet above.
* **UDP `sendData:` cannot be the C's one line.** This one is a behaviour
  change rather than a translation, so it is written up under
  [Divergences](#divergences) above.
* **The Mach-O link.** `aioEnable`/`aioHandle`/`aioDisable`/`aioFini` are
  undefined in the shared object by design, which ELF allows and `ld64` does
  not. The C plugin never noticed because CMake links it against the VM core
  library; corrosion builds this crate with a bare `cargo build`, so
  `build.rs` passes `-Wl,-U,_aio*` -- one symbol at a time, not
  `-undefined dynamic_lookup`, so a genuine typo in an extern is still a
  build error.

Two Darwin behaviours are *not* changed, because the C behaves the same way
there: `sqResolverLocalAddress` answers 0 (it looks for `eth0`/`wlan0`, which
macOS does not have), and the hand-built AF_UNIX `sockaddr_un` leaves
`sun_len` zero (the C has that assignment written out and commented away).

## Verification

`cargo test -p socket-plugin` runs 45 tests with no VM, of which 43 build on
Linux (two are Darwin-only). Those **43 were executed on
x86_64-unknown-linux-gnu**, against a VM built from this tree; the earlier
wave's were executed on aarch64-apple-darwin. The two Darwin-only tests remain
a compile-time claim on Linux, and this wave's six new ones remain one on
Darwin.

* **Pure functions**: net-address round-trips; the address-header
  validate/stamp protocol; port get/set on IPv4 and IPv6 sockaddrs; the
  option name table (including the 32-byte cutoff and interior-NUL
  truncation); a `strtol(_, _, 0)` clone tested against the C's exact
  "is this value all one integer?" decision.
* **The state machine, on real sockets**: a `cfg(test)` stand-in for `aio.c`
  (same one-shot handler contract, driven by an explicit poll) lets the
  loopback tests run create -> listen -> connect -> accept -> send -> receive
  -> close -> destroy through the connect/accept/data handlers, asserting
  every state transition, plus UDP datagram round-trips both by explicit
  destination and by connected peer, option get/set through a live socket,
  local/remote address-object round-trips, and session invalidation on
  network shutdown.
* **Layout**: the `SQSocket` size/offsets and the fd-first invariant of the
  private struct, plus where each platform puts `sa_family` inside
  `struct sockaddr` (offset 1 and one byte wide on Darwin, which still has
  4.4BSD's leading `sa_len`; offset 0 and two bytes on glibc).
* **The platform splits**: the AF_UNIX service-name shortcut end to end
  against a real bound socket -- which exercises `S_IFSOCK`, the `sun_path`
  bound and the `sockaddr_un` handed back, all three of which differ between
  the two; the `getaddrinfo`-failure arm each platform's `<netdb.h>` selects,
  including the both-arguments-empty failure `dns-lookup` produces without
  calling `getaddrinfo` at all; and, on macOS only, the two kernel rules
  behind `udp_send` plus the fact that its `getpeername` probe leaves `errno`
  as it found it.

* **The asynchronous resolver**, five tests that could not have been written
  before, because they assert what the resolver looks like *during* a lookup.
  A `#[cfg(test)]` gate that every worker takes for the whole of its body makes
  that a fact rather than a race -- a loopback `getaddrinfo` finishes in
  microseconds, so the in-flight window would otherwise be gone before the
  first assertion ran. They pin: `ResolverBusy` reported while the state mutex
  is demonstrably free; an abort disowning the worker so its answer is dropped
  and its doorbell never rings; a second lookup superseding the first by the
  same generation check; a shutdown not leaving `ResolverBusy` set for the next
  session; one doorbell per completed lookup in *both* directions; and the
  quiescence ledger `shutdownModule` reads, including the gap where no lookup
  is in flight but the thread that ran it is still alive.

The built `.so` was checked against the C plugin's export table: all 60
primitive names identical, all 60 accessor-depth bytes identical (40 zero, 20
minus-one), `getModuleName`/`setInterpreter`/`initialiseModule`/
`shutdownModule`/`moduleUnloaded` present, and `aio*` present as undefined
imports.

### Against a live image, Rust plugin versus C plugin

Two VMs were built from this tree on Linux x86_64 -- one with
`USE_RUST_PLATFORM=ON USE_RUST_PLUGINS=ON`, one all-C -- and the same
Pharo 12.0 image (build 1597, `4689a46372`) run on each:

| | C plugin | Rust plugin |
|---|---|---|
| `primStartLookupOfName:` returns after | 2,384 us | **93 us** |
| `resolverStatus` immediately after | 1 (`ResolverReady`) | **2 (`ResolverBusy`)** |
| Smalltalk loop iterations during the query | **0** | **4,049,919** (over 155 ms) |
| `addressForName: 'files.pharo.org'` | `193.49.213.186` | `193.49.213.186` |
| `nameForAddress: 8.8.8.8` | `'dns.google'` | `'dns.google'` |
| `primAbortLookup` while busy | n/a (never busy) | 2 -> 1, immediately |
| `Smalltalk vm unloadModule:` with a lookup in flight | unloads | **refused** (primitive fails); the lookup then completes |
| `Smalltalk vm unloadModule:` once quiescent | unloads | unloads, and re-`dlopen`s correctly on the next lookup |
| `Socket newTCP connectToHostNamed: 'files.pharo.org' port: 80` + `GET /` | `HTTP/1.1 301 Moved P...` | `HTTP/1.1 301 Moved P...` |

The last row is the regression check that matters most and reads as the least
interesting: `connectToHostNamed:` resolves through the asynchronous path and
then drives the socket half unchanged, so an end-to-end HTTP request over a
real network answers byte for byte what the C plugin answers.

**And what it costs.** One Process's lookup gets *slower* end to end, and the
tables above would be dishonest without the number: 200 back-to-back
`NetNameResolver addressForName:` calls for a name the OS resolver has already
cached average **1,644 us on the C plugin and 1,997 us on this one**. The extra
~350 us is a thread spawn, a doorbell round trip and a scheduler wake — the
p50 of (d) plus the spawn. That is the trade, made deliberately: the Process
doing the lookup waits about a fifth longer, and every other Process in the
image stops waiting at all.


The third row is the change stated as a user would feel it: on the C plugin the
image executes *nothing* while a name is resolved; on this one it executed four
million bytecoded loop iterations, in the same process, and then got the same
answer. Both VMs were also checked to be identical on the failure the image hit
first -- a `git_libgit2_init` symbol lookup against the libgit2 this build
ships, which fails on the all-C VM in exactly the same way, so it is a
packaging problem in this build tree and not a platform-layer regression.

## Not verified

The suite has not been run on Linux in this wave -- only cross-compiled and
linted -- so the Linux arms of the three `cfg` splits (`s_ifsock`,
`gai_error_is_fatal`, `udp_send`) are unchanged code that nothing here
re-executed.

The image-side differential above covers the resolver primitives, the module
handshake and the resolver semaphore's delivery from a foreign thread. What it
does not reach, and a fuller pass still should:

* **Load and stack discipline** for the other 40 primitives: the literal
  `pop`/`popthenPush` counts against a live interpreter.
* **Semaphore delivery** for the connect/read/write notifications through the
  real aio poll loop (only the resolver semaphore was exercised).
* **RAW sockets** (`SOCK_RAW`/ICMP needs root) and the **provided-socket**
  type.
* **The AF_UNIX local-socket `getaddrinfo` path** beyond the unit test: the
  resulting address actually being connected to.
* **Reverse DNS** success paths beyond the one live lookup above --
  environment-dependent in a unit test, so only the error contract is pinned
  there.
* **The resolver under contention**: two Processes racing
  `NetNameResolver`'s own `resolverMutex` is what serialises lookups image-side,
  and nothing here drives a second Process past it.
* **Interop with `UnixOSProcessPlugin`** reading the descriptor through the
  private pointer (the layout invariant is asserted, the interop is not run).
* **Windows.** Not ported: the crate is Unix-only, like `sqUnixSocket.c`
  before the `_WIN32` blocks were merged in. The C's Windows build linked
  `ws2_32`; a Windows port would be a separate effort.

Dead code was not carried over: `sqSocketSetReusable`,
`sqSocketSendUDPToSizeDataBufCount` and the two-argument
`sqSocketReceiveUDPDataBufCount` are declared in the C header but reachable
from no primitive and referenced nowhere else in the tree.

## Not done yet

* **The 2007 `getaddrinfo` API is still synchronous** and still stops the
  image for the length of a lookup, exactly as the classic path used to. The
  primitives are the same shape, so the machinery above transfers unchanged --
  what does not transfer is the image side, which reads
  `primitiveResolverGetAddressInfoSize` straight back without waiting on a
  semaphore. That needs an image change, and therefore a decision about
  whether to make one.
* **macOS.** Everything above was measured on Linux. The resolver change is
  platform-neutral, but the numbers, and the foreign-thread signalling they
  rest on, have not been reproduced on Darwin.
