# AioPlugin

Descriptor readiness and OS-resolution timers, as Pharo primitives, on the poll
loop the VM already runs.

An image that wants to know when a file descriptor becomes readable has one
option today: poll it from a Process at whatever granularity a `Delay` gives.
An image that wants a timer finer or more accurate than the `Delay` machinery
has none. Both are already solved inside the VM — `src/unix/aio.c` (epoll) and
`src/osx/aioOSX.c` (kqueue) wait on a set of descriptors on every idle turn, and
`aioEnable` / `aioHandle` / `aioDisable` are exported — and until now nothing
reached them but `SocketPlugin`.

So this plugin is thin on purpose: **no reactor, no runtime, no threads.** It
registers a descriptor with the loop that is already running and rings an
image-side semaphore from the handler. The reactor rewrite `CLAUDE.md` describes
— a persistent `mio::Poll` replacing the `epoll_create1` / N × `epoll_ctl` /
`epoll_wait` / `close` the VM does **per poll** — is a separate, later,
Linux-first wave, and this deliberately does not wait for it.

## The contract

| primitive | answers |
|---|---|
| `primitiveAioWaitFd: fd events: mask signalling: semaIndex` | a watch handle |
| `primitiveAioTimerAfter: milliseconds signalling: semaIndex` | a watch handle |
| `primitiveAioResult: handle` | the events that fired, retiring the watch; `-1` while pending |
| `primitiveAioCancel: handle` | `true` if it retired one, `false` if the handle named nothing |
| `primitiveAioOutstanding` | how many watches are live |

`mask` and the result use `aio.h`'s bits, which the image should mirror:
**1 exception, 2 readable, 4 writable.** A result may carry the exception bit
alongside the one asked for. `semaIndex` is an index from
`Smalltalk registerExternalObject:`; zero means "do not signal".

**Watches are one-shot.** `aio.c` clears a descriptor's mask before calling its
handler, and this plugin never re-arms, so an event is delivered exactly once
and the image asks again if it wants another. That is the edge-triggered loop it
is meant to be used in:

```smalltalk
| sema index handle |
sema := Semaphore new.
index := Smalltalk registerExternalObject: sema.
[ true ] whileTrue: [
    handle := nil aioWaitFd: fd events: 2 signalling: index.
    sema wait.
    (nil aioResult: handle) = 2 ifTrue: [ "read from fd" ] ]
```

A handle must be collected or cancelled. `primitiveAioResult:` retires the watch
when it answers a real result, so the loop above leaks nothing; a Process that
gives up calls `primitiveAioCancel:`, which is safe to call unconditionally.

## Three things worth knowing before using it

**Events arrive on the VM's *idle* turn.** The poll loop runs from
`ioRelinquishProcessorForMicroseconds`, so a Process that spins without ever
letting the VM go idle starves it and no watch ever fires. This is a property of
the VM rather than of this plugin — `SocketPlugin`'s notifications behave
identically — but it is the first thing to get wrong. A `Processor yield` loop
is not idle; a `Delay` is.

**It never touches a descriptor the image owns.** Every registration passes
`AIO_EXT`, which is precisely why `aioEnable` does not set `O_NONBLOCK | O_ASYNC`
on it or `F_SETOWN` it to this process, and why `aioFini` will not close it. A
`cfg(test)` assertion pins that: one argument in one call is exactly the kind of
thing that gets dropped in a refactor and noticed a year later by whoever's
socket stopped blocking. Timer descriptors are this plugin's own and are closed
when their watch retires.

**It refuses to be unloaded while a watch is armed.** `aio.c` keeps the
handler — a function pointer into this library's text — in its `descriptorList`
keyed on fd, and only `aioDisable` takes it out, so `shutdownModule` answers 0
until the registry is empty. That is `CLAUDE.md`'s quiescence rule applied to
the *other* way a plugin hands a pointer outward: not a thread of its own, but a
callback the VM is holding.

## How a timer is a descriptor

The VM already knows how to wait for a descriptor, so a timer only has to be
something that becomes readable when it expires.

* **Linux**: `timerfd_create(CLOCK_MONOTONIC, TFD_NONBLOCK | TFD_CLOEXEC)` with
  a zero `it_interval`, which is what makes it one-shot. A request for 0 ms
  becomes 1 ns, because a zero `it_value` *disarms* a timerfd rather than firing
  it — a test pins that distinction.
* **kqueue platforms**: a kqueue whose only registered event is an
  `EVFILT_TIMER` with `EV_ONESHOT`. A kqueue descriptor is readable exactly when
  it has an event pending, so a kqueue holding one timer *is* a timer
  descriptor.

Neither is ever read from: a watch fires once, is not re-armed, and its
descriptor is closed on retirement, so draining it would be work with no
observer.

The delay a caller observes is the requested one plus the poll loop's own
latency, which is bounded by how long the VM is willing to sit in `epoll_wait`
and is not this plugin's to promise.

## Handles, not pointers

The image gets a SmallInteger handle from `pharo_vm_plugin::handles` — carrying
a slot index, a generation, a type tag and a session byte — and **that same
integer, not a pointer, is what goes to `aioEnable` as its `clientData`.**

The C plugins put a `struct *` there and the handler dereferences it, which is a
use-after-free the moment a descriptor is disabled while an event for it is
already in flight. An integer cannot dangle: a stale one fails to resolve and
the handler does nothing. A test pins that a handle from a retired watch does
not resolve to whatever took its slot.

## Verification

`cargo test -p aio-plugin` runs 8 tests with no VM, executed on
`x86_64-unknown-linux-gnu`. They drive watches end to end against a `cfg(test)`
poll loop with the same one-shot contract as `aio.c`, dispatched by an explicit
turn instead of by the interpreter's idle path: a readable pipe firing once and
ringing exactly one doorbell and then not firing again; a writable descriptor
firing for write and not for read, so the mask is honoured rather than ignored;
a timer firing and closing its own descriptor while an image-owned one is left
open; a cancelled watch delivering nothing; a stale handle not resolving to the
next watch; and the quiescence condition `shutdownModule` refuses on.

`aarch64-apple-darwin` is **cross-checked, not run** — `cargo check --target`,
verified to be really compiling the kqueue branch by breaking it on purpose and
watching the check fail. No kqueue machine was available.

### Against a live image

Linux x86_64, Pharo 12.0 build 1597, on a VM built from this tree with
`USE_RUST_PLATFORM=ON USE_RUST_PLUGINS=ON`:

```
timer armed, outstanding = 1
timer: waited 250 ms for 250, events=2, a background Process ran 25 times meanwhile
outstanding after collecting = 0
fd 0 watch armed, immediate result = -1 (-1 means still pending), outstanding = 1
fd 0 became readable after 522 ms, events = 2
outstanding at the end = 0
a minute-long timer: outstanding = 1, cancel answers true, cancel again answers false, outstanding = 0
```

The fd watch is on standard input, with the script run as
`( sleep 1; echo hello ) | pharo …`, so the readiness edge happens at a moment
nothing in the image chose. The 250 ms timer landed on 250 ms while another
Process kept running.

## Not verified

* **The primitives' own argument handling and stack discipline.** The unit tests
  go through the watch registry rather than the primitives, because a primitive
  needs an `&Interp` and there is no VM in a unit test. The live run above
  exercises the happy path of all five; the refusals (a negative fd, an events
  mask with bits outside `AIO_R|AIO_W|AIO_X`, a delay past `MAX_DELAY_MS`) are
  not driven from an image.
* **kqueue, at all.** See above.
* **Windows.** `src/win/aioWin.c` has its own aio and this crate has not been
  written against it; `cmake/rust.cmake` builds this plugin only on `UNIX`.
* **Descriptor exhaustion.** `primitiveAioTimerAfter:` answers `Unsupported`
  when the kernel refuses a descriptor; nothing has driven the image into that
  state to watch it happen.
* **Many watches at once.** The VM rebuilds its epoll set on every poll, so a
  large registry has a cost this plugin does not control and nothing here has
  measured. That is the reactor rewrite's problem, and the reason it exists.
