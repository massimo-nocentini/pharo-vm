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

`cargo test -p socket-plugin` runs 38 tests with no VM (36 on Linux: two are
Darwin-only). Those 38 were executed on aarch64-apple-darwin only. For Linux
this wave ran `cargo check` and `cargo clippy --all-targets` for
`aarch64-unknown-linux-gnu` from the same Mac, both clean; nothing was
executed on Linux, so the 36 remain a compile-time claim there.

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

The built `.so` was checked against the C plugin's export table: all 60
primitive names identical, all 60 accessor-depth bytes identical (40 zero, 20
minus-one), `getModuleName`/`setInterpreter`/`initialiseModule`/
`shutdownModule`/`moduleUnloaded` present, and `aio*` present as undefined
imports.

## Not verified

The suite has not been run on Linux in this wave -- only cross-compiled and
linted -- so the Linux arms of the three `cfg` splits (`s_ifsock`,
`gai_error_is_fatal`, `udp_send`) are unchanged code that nothing here
re-executed.

No VM runs in this environment, so an image-side differential pass should
focus on:

* **Load and stack discipline**: the literal `pop`/`popthenPush` counts and
  the module handshake, against a live interpreter.
* **Semaphore delivery**: connect/read/write notifications and the resolver
  semaphore reaching image-side processes through the real aio poll loop.
* **RAW sockets** (`SOCK_RAW`/ICMP needs root) and the **provided-socket**
  type.
* **The AF_UNIX local-socket `getaddrinfo` path** beyond the unit test: the
  resulting address actually being connected to.
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
