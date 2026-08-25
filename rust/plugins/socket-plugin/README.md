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
`ioLoadFunctionFrom` indirection because the C never used one either. The
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
  the C's Linux build did. (The macOS C build behaved differently; this port
  uses the Linux semantics everywhere.)

## Verification

`cargo test -p socket-plugin` runs 32 tests with no VM:

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
  private struct.

The built `.so` was checked against the C plugin's export table: all 60
primitive names identical, all 60 accessor-depth bytes identical (40 zero, 20
minus-one), `getModuleName`/`setInterpreter`/`initialiseModule`/
`shutdownModule`/`moduleUnloaded` present, and `aio*` present as undefined
imports.

## Not verified

No VM runs in this environment, so an image-side differential pass should
focus on:

* **Load and stack discipline**: the literal `pop`/`popthenPush` counts and
  the module handshake, against a live interpreter.
* **Semaphore delivery**: connect/read/write notifications and the resolver
  semaphore reaching image-side processes through the real aio poll loop.
* **RAW sockets** (`SOCK_RAW`/ICMP needs root) and the **provided-socket**
  type.
* **The AF_UNIX local-socket `getaddrinfo` path** (stat-based).
* **Reverse DNS** (`gethostbyaddr`) success paths -- environment-dependent
  here; only the error contract is unit-tested.
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

* **CMake wiring.** The crate builds and exports the right surface, but the
  build still compiles the C plugin. Switching over means adding this crate
  to `cmake/rust.cmake` and dropping `SocketPlugin` from
  `cmake/plugins.cmake` (keeping `UnixOSProcessPlugin`'s include path to the
  `SQSocket` header) -- deliberately left as a separate change, so the swap
  is reviewed on its own.
