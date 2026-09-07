# Working in this repository

A fork of the Pharo VM whose hand-written C platform layer (`src/`) and sixteen
C plugins (`plugins/`) are being replaced by Rust under `rust/`. The
interpreter, GC and JIT are generated from Slang (`smalltalksrc/`) and are out
of scope — hand-porting them would fork the VM off VMMaker and lose the
simulator.

`docs/rust-port.md` records where the migration stands. `rust/README.md` holds
the build integration and the house rules. Read both before changing anything
under `rust/`.

## Build and test

Two cargo workspaces, and the split is load-bearing:

```
rust/Cargo.toml           pharo-platform, pharo-vm-sys, the SDK, macros   panic = "abort"
rust/plugins/Cargo.toml   the plugin cdylibs                              panic = "unwind"
```

Cargo reads `[profile]` only from a workspace *root* — a `[profile]` in a
member is ignored with a warning — and the platform layer must abort (it is
linked into `libPharoVMCore` with no catch above it, and is reached from the
heartbeat and from signal handlers) while the plugins must unwind (every
primitive body is fenced by `catch_unwind` in `run_primitive`). Two
requirements, one knob per workspace, hence two workspaces.

```sh
cd rust/plugins && cargo test --workspace --locked          # 634 tests, 53 binaries
cd rust && source ../build/rust-env.sh && cargo test --workspace --locked   # 184 tests
```

**`pharo-vm-sys` refuses to build standalone by design** — it binds headers
whose types depend on the `config.h` CMake generates — so the outer workspace
needs `source build/rust-env.sh` first. Without it you get a build-script panic
that looks like a regression and is not.

On a machine with no Linux, prove Linux with
`cargo check --target aarch64-unknown-linux-gnu --all-targets --locked`
(exclude `squeak-ssl` in the plugins workspace — its vendored OpenSSL will not
cross-compile). Say "cross-checked", never "passes on Linux", in a README.

**On a Linux box, build both VMs and diff them — it is cheap and it is the only
thing that settles anything.** A full configure and build takes a few minutes:

```sh
cmake -S . -B build   -DCMAKE_BUILD_TYPE=Release -DUSE_RUST_PLATFORM=ON  -DUSE_RUST_PLUGINS=ON \
      -DPHARO_DEPENDENCIES_PREFER_DOWNLOAD_BINARIES=TRUE -DICEBERG_DEFAULT_REMOTE=httpsUrl
cmake --build build -j
```

For the all-C comparison build, reuse the generated sources
(`ln -s ../build/generated build-c/generated`, then `-DGENERATE_SOURCES=OFF
-DGENERATE_VMMAKER=OFF`) and turn off the two plugins whose C versions need
system development packages the Rust build vendors away:
`-DFEATURE_PLUGIN_SSL=OFF -DFEATURE_PLUGIN_UUID=OFF`. Then:

```sh
rust/tools/abi-check.sh capture build-c/build/vm/libPharoVMCore.so /tmp/abi.txt
rust/tools/abi-check.sh compare build/build/vm/libPharoVMCore.so   /tmp/abi.txt
```

Two things about running an image on such a build, both of which cost an hour
to find and neither of which is a port regression:

- The `st` command-line handler **hangs** on a headless Pharo 12.0 image before
  it runs a line of the script — on the stock `files.pharo.org` VM too. Use
  `eval 'Smalltalk compiler evaluate: ''<abs path>.st'' asFileReference contents'`.
- The build ships a `libgit2` whose `git_libgit2_init` the image fails to
  resolve, which aborts startup before any command runs. Identical on the
  all-C build. Move `build/build/vm/libgit2.so*` aside for a test run.

## Invariants

- **Faithfulness to the C.** The Rust reproduces what the C did, bugs included.
  Every intentional difference is documented under a **Faithful oddities**
  (a bug kept) or **Divergences** (a bug fixed) heading in the module docs and
  the crate README. A plausible, POSIX-correct implementation that differs from
  the C is a bug here, because the image was written against the C.
- **Exported symbols are pinned** by `rust/tools/abi-check.sh`. Do not add,
  remove or rename a `#[no_mangle]`. It filters `^rust_`, so a new
  `rust_`-prefixed platform export is the one safe shape.
- **No `unwrap()`/`expect()`/`panic!`/indexing reachable from image or network
  input.** A panic in a primitive now fails the primitive; a panic in a
  C-sandwiched callback still aborts.
- **Citations are the audit trail.** Cite committed sources. Do not cite
  `build/generated/**/cointerp.c`: it is gitignored and its line numbers differ
  between the 32- and 64-bit builds. Cite the Slang under `smalltalksrc/`.
- **rustfmt debt is pre-existing and deliberate.** ~239 hunks. Do not blanket-run
  `cargo fmt`; keep only newly written code clean. `cargo fmt --check` is not in
  CI because it has never passed.

## The shape every new capability must take

Rust cannot call into the image: `ptEnterInterpreterFromCallback` returns via
`siglongjmp`, which must not cross a Rust frame. A foreign thread may not read
an oop, allocate, or answer a value — its entire vocabulary is "increment a
counter in the request table". A primitive that blocks, blocks the GC.

So there is one legal shape:

> The image names a Rust-owned thing with a SmallInteger. Work happens where
> the image is not. A counted signal wakes a Process. A later primitive, on the
> VM thread, copies bytes out. — **handle, doorbell, collect.**

`Interp::signal_semaphore` (`rust/pharo-vm-plugin/src/interp.rs:889`) is the
doorbell. Signal through it, never through the `Semaphore` vtable:
`pharo_semaphore_signal` reads `failed()`, which is interpreter state.

## Remaining work, in order

**Start at §4** — and note that what is left of it is one bug fix, not a
feature. §1 was measured to be unbuildable on glibc; §2 and §3 are built. All
three are kept for what they record rather than for work they still name.

### Done: the foreign-thread edge, and async DNS on it

Both landed together, and what they established is what everything below rests
on. Read this before designing anything asynchronous.

**Foreign-thread signalling works, and is bounded.** `rust/examples/ext-sem-soak`
is the throwaway plugin this work required first: N threads signalling a contiguous
block of registered indices at a paced rate, driven by `soak.st` against a
GC-heavy image. Measured on Linux x86_64 against a Pharo 12.0 image (the numbers
and the method are in `docs/rust-port.md`, Wave 13):

- **No lost signals.** 8 threads, ~71.7k signals/s, ten minutes: **43,019,960
  sent, 43,019,960 received**, matched per index. The counting request table
  holds at rate.
- **`signalSemaphoreWithIndex`** costs mean 4.8 us / p99 18.9 us / max 280 us
  as a signaller sees it, `requestMutex` contention included — over the first
  131,072 samples of the run, not a uniform draw, so read the tail as
  indicative rather than as a bound.
- **`sigprocmask` is per-thread on glibc**, pinned by a unit test in
  `external_semaphores.rs`. Darwin is still unverified — run the soak there
  before trusting it.
- **Wake latency** is mean 55 us, p50 38 us, p99 766 us, max **7.19 ms**, end
  to end over 2,000 paired sends. "Worst case one relinquish quantum" holds for
  the body of it — but the max is three orders of magnitude above the median,
  so anything that needs a *bound* rather than a typical case does not have one
  from this.

Ten minutes, not the hour that was asked for, and one machine on one
platform. Run it longer, and run it on Darwin, before leaning harder on it.

**The one result that constrains every design below: a signaller must be
paced.** With no pause at all the image makes *no progress whatsoever* — eight
threads at 700% CPU and a five-second run unfinished after two minutes —
because `signalSemaphoreWithIndex` calls `forceInterruptCheck()` and
`aioInterruptPoll()` on **every** signal. So §3's "one external-semaphore index
for the whole runtime, one pump Process" is a measured requirement, not a
preference, and any design that signals per-event needs an answer to "how many
events per second". `SOAK_MICROS=0` is kept as the starvation probe.

**Async DNS shipped on that edge.** `NetNameResolver class >> addressForName:timeout:`
-- and `Socket>>connectToHostNamed:port:` above it -- no longer freezes the
image: `primStartLookupOfName:` returns in ~93 us instead of ~2,400 us, answers
`ResolverBusy`, and the image executed 4,049,919 bytecoded loop iterations
during a 155 ms query where the C plugin executed none. No image-side change
was needed — `NetNameResolver` was written for the asynchronous contract all
along. The shape, in `rust/plugins/socket-plugin/src/resolver.rs`: `lastError`
and a busy flag are atoms read lock-free (the image polls status in a loop and
must never queue behind a worker); a generation counter bumped under the mutex
decides who may commit, which is what finally makes `sqResolverAbort` mean
something; the mutex is held only for the microseconds it takes to store an
answer.

**What it costs, since a wave that only reports its wins is not an audit
trail.** One Process's lookup is *slower* end to end: 200 back-to-back
`addressForName:` calls for a cached name average 1,644 us on the C plugin and
1,997 us here — a thread spawn plus a doorbell round trip plus a scheduler
wake. The Process doing the lookup waits about a fifth longer; every other
Process in the image stops waiting at all.

**And the trap it walked into, which generalises.** `shutdownModule` answered 1
unconditionally, so `Smalltalk vm unloadModule:` could `dlclose` the library out
from under a worker parked in `getaddrinfo`. That was correct until the plugin
had a thread. socket-plugin now carries the quiescence ledger §1 below
describes, and it is the worked example to copy — including the refinement that
matters: **the ledger holds `JoinHandle`s, not a count.** A counter decremented
at the end of the worker's closure reaches zero while the thread is still
running libstd's own epilogue and its TLS destructors, every byte of which is
code in that cdylib; `join` is the only thing that means "this thread is gone".
Note also the gap a test pins: after an abort there is no job in flight and the
thread that ran it is still alive.

Verified live, and it exercises §1 as a side effect: with a lookup in flight
`Smalltalk vm unloadModule: 'SocketPlugin'` fails the primitive and the lookup
then completes normally; once quiescent it succeeds, and the module is
re-`dlopen`ed on the next primitive and answers correctly. That is the reload
loop working — it does not prove `dlclose` *unmapped* anything, which still
needs the version-stamping primitive §1 asks for.

**What async DNS did not do.** The 2007 `getaddrinfo` API
(`primitiveResolverGetAddressInfo` and friends) is still synchronous and still
stops the image. The machinery transfers unchanged; the image side does not —
it reads the results straight back without waiting on a semaphore — so that one
does need an image change, and therefore a decision.


### 1. Hot-reloading Rust plugins — **does not work on glibc. Measured. Do not start here.**

The idea was: primitive 571 `Smalltalk vm unloadModule:` already runs
`shutdownModule`, broadcasts `moduleUnloaded`, `dlclose`s, removes the entry and
flushes external primitives; a C plugin was never worth reloading because
rebuilding meant a full VM build, but a Rust crate rebuilds in seconds. So
unload, `install` a new build, and the next primitive call runs new code in the
same image.

**It does not.** The version-stamping experiment this item asked for has now
been run, on Linux x86_64/glibc, with a throwaway plugin whose only primitive
answers a compile-time version number. Five variants, each: install v1, do one
thing, `unloadModule:`, install v2 over it with a *new inode*, ask the version.

| what the plugin did first | mappings after unload | next call answers |
|---|---|---|
| nothing at all | **0** | **v2 — reloaded** |
| touched one `thread_local!`, on the VM thread, once | 4 | v1 — **stale** |
| spawned and joined a thread touching none of its own TLS | 4 | v1 — **stale** |
| both | 4 | v1 — **stale** |
| **one `poison::Section::enter()` and nothing else** | 4 | v1 — **stale** |

The last row is the one that kills it. `Section` is the SDK's, `poison::lock`
opens one for every guarded mutex and `handles::Registry` opens one for every
lock it takes — and the SDK is statically linked into *every* plugin cdylib, so
its `thread_local!` lives in each one. Any plugin that takes a lock or touches a
registry — which is all of them, on their first substantive primitive — is
pinned for the life of the process.

Two details worth keeping. The trigger is **TLS instantiation in the DSO**, not
threads: touching a thread-local once on the interpreter thread is enough, and
it is permanent, so this is not the `l_tls_dtor_count` race this item used to
guess at. And the mapping count stays at four rather than going to eight, so
`dlopen` is handing back the *retained* object rather than mapping the new file:
glibc's `_dl_map_object` matches an already-loaded object by **name**, so a new
inode at the same path changes nothing.

What that leaves:

* **The unload half is real and worth having.** `ioUnloadModule` honours a
  `shutdownModule` of 0, and socket-plugin's quiescence ledger (below) makes
  that refusal meaningful. Unloading a module that never ran is a genuine
  `dlclose`.
* **Reload would need a different name, not a different file.** Loading
  `libFooPlugin2.so` as module `FooPlugin2` does get new code — but the image's
  `<primitive: 'x' module: 'FooPlugin'>` pragmas name the module, so this is
  only useful to something willing to rewrite pragmas. That is a real design,
  and a different one from this item.
* **macOS is unmeasured.** dyld's rules differ; run the same probe before
  assuming either answer.

So: do not build the `reloadable = […]` macro. The rest of this item — the
quiescence ledger — stands on its own and is already built.

socket-plugin is the worked example: its async resolver made it the first
plugin in the tree that can outlive a primitive, and `shutdownModule` answering 1 unconditionally would have let
`dlclose` unmap the text a worker was executing. It now answers 0 while any
worker `JoinHandle` is unjoined. **Hold handles, not a count**: a counter
decremented at the end of the worker's closure reaches zero while the thread is
still running libstd's epilogue and its TLS destructors, all of which is code in
that same cdylib. Copy that shape.

**The safety condition is not "registries are empty".** Plugins hand function
pointers into their own text outward through channels the SDK cannot see:
`aioHandle` stores a handler in `aio.c`'s `descriptorList` keyed on fd;
`addSynchronousTickee`/`addHighPriorityTickee` take one of four slots each;
`Semaphore` vtables sit inside any TSQueue built; `RETAINED_PINS` holds oops
that could not be released. So `pharo_plugin!(… reloadable = […])` must
generate a `shutdownModule` answering 0 unless a full **quiescence ledger** is
empty. `ioUnloadModule` already honours that refusal
(`rust/pharo-platform/src/named_prims.rs:957-959`: `shutdown_module(entry) == 0`
→ return 0).

Use `install`, never `cp` — writing into a mapped inode is SIGBUS.

Unloading is also the only path in the tree that ever runs a shutdown hook, so
it remains the first thing to give shutdown code any coverage — which cuts both
ways.

### 2. fd watches and timers — **done**, in `rust/plugins/aio-plugin`

`AioPlugin` is a rust-only plugin over the poll loop the VM already runs. No
reactor, no runtime, no threads: `primitiveAioWaitFd:events:signalling:` and
`primitiveAioTimerAfter:signalling:` register a descriptor with `aioEnable` /
`aioHandle` and ring a semaphore from the handler, plus `primitiveAioResult:`
to collect, `primitiveAioCancel:` and `primitiveAioOutstanding`. Measured live:
a 250 ms timer landed on 250 ms while another Process kept running, and an fd
watch on stdin woke at the moment a pipe was written.

Four things that came out of building it, all of which generalise:

* **Events arrive on the VM's *idle* turn.** The poll loop runs from
  `ioRelinquishProcessorForMicroseconds`, so a Process that spins without
  letting the VM idle starves it and no watch ever fires. A `Processor yield`
  loop is not idle; a `Delay` is. True of `SocketPlugin`'s notifications too,
  and undocumented until now — it cost a hung test to find.
* **A watch is a quiescence hazard even with no threads.** `aio.c` holds the
  handler — a pointer into the plugin's text — in its `descriptorList` keyed on
  fd, and only `aioDisable` takes it out. So `shutdownModule` answers 0 while
  any watch is armed, for the same reason socket-plugin's does with a worker
  alive. This is the "channels the SDK cannot see" case §1 lists, made real.
* **Put the handle in `clientData`, never a pointer.** The C plugins put a
  `struct *` there and dereference it in the handler, which is a use-after-free
  the moment a descriptor is disabled with an event already in flight. A
  SmallInteger handle cannot dangle: a stale one fails to resolve and the
  handler does nothing.
* **A timer is just a descriptor that becomes readable.** `timerfd` on Linux, a
  kqueue holding one `EVFILT_TIMER` on the BSDs. Note that a zero `it_value`
  *disarms* a timerfd rather than firing it, so "after 0 ms" has to become 1 ns.

The reactor rewrite (a persistent `mio::Poll` replacing the
`epoll_create1`/N×`epoll_ctl`/`epoll_wait`/`close` **per poll**) is still a
separate, later, Linux-first wave: it touches `libPharoVMCore`, three platform
files and the heartbeat poll handshake. It did not hold this item hostage, and
it is the reason many simultaneous watches have a cost nothing here has
measured.

### 3. AsyncPlugin — **done**, in `rust/plugins/async-plugin`

A rust-only cdylib holding a pool, a task registry, a queue of completed handles
and **one** external-semaphore index for the whole runtime. The image runs one
pump Process and never sees a semaphore. Jobs: read a file, write a file, sleep.
Measured live, the same 40 MB read both ways:

```
40 MB blocking read froze the image for 96 ms
40 MB async read finished in 126 ms while a background Process ran 3,987,078 times
```

Not tokio, and the reason generalises: every job here *blocks*, an async
runtime's value is multiplexing many waits onto few threads, and under tokio all
of this would go to `spawn_blocking` — a thread pool. Genuinely async work has
somewhere to go already: sockets to `SocketPlugin` and the VM's poll loop,
timers to §2's `AioPlugin` on that same loop with no threads at all.

The three traps held up. Two need amending:

1. **The drain must be peek-and-ack, not pop** — and **the ack is mandatory**,
   which the original note left out. `IntoReturn` runs *after* the body and an
   allocation failure makes the interpreter scavenge and re-run the primitive,
   so a pop would lose the completion; peeking is right. But a completion the
   image cannot collect then sits at the head of the queue forever and a pump
   without an error arm spins on it. It did, on the first live run. A pump
   either collects a handle or cancels it. A test pins the property rather than
   removing it, because it is what makes the re-run safe.
2. **`Registry::with` holds the mutex across the closure** — confirmed, and the
   `Arc` needs one wrapper: coherence refuses `impl Resource for Arc<Task>`
   because both the trait and `Arc` are foreign. A newtype costs one line.
   A test asserts 1,000 registry reads finish well inside the length of a job
   running at the time, which is what "no worker holds the lock" actually means.
3. **`shutdownModule` must refuse while any job is outstanding** — done, and it
   *joins* its workers rather than counting them down, for the reason §1
   measured.

And one the list did not have: **a plugin cannot allocate a large object.**
`instantiateClass:indexableSize:` allocates out of what the image already has
and does not grow the heap; the interpreter retries after a scavenge and after a
full GC, then gives up. On a stock Pharo 12.0 image 1, 4 and 16 MB collect while
32 and 40 MB fail — headroom, not a constant, since 16 MB fails once the image
is already holding another 16. Anything larger has to be chunked. The failure is
clean *because* the drain peeks: the task stays done, the result intact and the
handle still queued, which was verified live and is trap 1 paying for itself.

### 4. "Run the VM off the main thread" — mostly already true

`run_on_worker_thread` (`rust/pharo-platform/src/client.rs:283`) already spawns
the interpreter via `std::thread::Builder`, named `"pharo-vm"`, with **4× the
platform default stack** read from `pthread_attr_getstacksize`.
`PHARO_VM_IN_WORKER_THREAD` is `ON` (`CMakeLists.txt:52`); the runtime switch is
`--worker`.

The real question is what the freed main thread does, and today it does one
thing: `runMainThreadWorker()` blocked in `sem_wait` on a TSQueue.
`EXPORT(Worker*) mainThreadWorker` is referenced nowhere else in `src/` — it
exists purely so the image can find it. **The version that works is ten lines**:
a `primitiveAttachMainThreadWorker` that `setHandler`s it onto a
`TFMainThreadWorker`, failing `Unsupported` when null. Every `TFExternalFunction`
invoked with that worker then runs on OS thread 0 — SDL, AppKit, GTK.

Fix first, because it is a process-exit hazard reachable from ordinary image
code: `TFWorker>>release` on the main-thread worker queues `WORKER_RELEASE`;
teardown frees the `Box`; `worker_run` returns to `runMainThreadWorker` →
`run_on_worker_thread` → `vm_main_with_parameters` → **the process exits by
returning from `main` while the detached interpreter thread is still running
image code**, leaving the exported global dangling. Add an `is_main_thread`
field to `Worker` (safe: only the leading `Runner` is ABI, and `worker.h` never
defines the struct) and skip both arms.

**Do not fill `scheduleInMainThread`.** Its declared signature is synchronous,
which invites the deadlock where the VM thread waits on the main thread while
the main thread's worker services a callout that needs the VM. Route through the
worker queue as an ordinary CALLOUT instead.

Note what `--worker` does *not* give you: a thread parked in `sem_wait` pumps no
run loop. SDL may work (it pumps Cocoa inside `SDL_PumpEvents`); `[NSApp run]`
will not. Nothing in the tree exercises this — verify empirically.

### What cannot be built, and why it keeps being proposed

`waitOnExternalSemaphoreIndex` (`rust/pharo-platform/src/external_semaphores.rs:455`)
is implemented, exported, wired into the proxy, and called by **nothing**. It
cannot make an async result look synchronous from Rust, for two independently
fatal reasons:

1. Its return path ends in an enilopmart or `siglongjmp`, straight through the
   Rust frame. Any live `lent::Lease` never runs `Drop`, and that table is a
   `thread_local` of exactly **8** slots — eight awaits and every in-place view
   in the process fails forever, with no error pointing at the cause.
2. Even from a correct C shim it cannot answer the awaited value: a blocking
   primitive must place its result on the stack *before* the switch, because
   after it the primitive never resumes. `primitiveWait` gets away with popping
   nothing only because it is zero-argument.

So `^ RsDns lookup: 'pharo.org'` answering an Array is not implementable short
of a Slang change. Use `sema wait` plus a fetch primitive.

## Open questions

- **`sigaltstack` in unix-os-process-plugin.** The C requests the alternate
  stack with the queried `SS_DISABLE` and therefore never installs one, while
  still answering `true`. The Rust deliberately does *not* copy that: matching
  the C would change shipped Linux signal handling, which a porting wave has no
  business doing. Deciding to match it needs its own commit and a JIT-VM
  differential run.
- **FilePlugin on macOS.** Still `if(UNIX AND NOT APPLE)`. `sq2uxPath`/
  `ux2sqPath` ignore the `norm` argument (`charconv.rs` names it `_norm`) that
  the C's `CFStringNormalize` branch honours, so paths do not round-trip on
  HFS+. `MAXPATHLEN`/`PATH_MAX` are hard-coded 4096 against Darwin's 1024. Note
  FileAttributesPlugin resolves `sq2uxPath` at *run* time via
  `ioLoadFunctionFrom`, so it currently inherits the C plugin's normalisation —
  un-gating FilePlugin without the branch silently changes that plugin too.
- **SqueakSSL.** Needs a SecureTransport backend for macOS (`src/osx/sqMacSSL.c`
  is 875 lines of a different TLS stack with a different trust model and a
  different meaning for `SQSSL_PROP_CERTNAME`). Independently: the vendored
  OpenSSL has `OPENSSLDIR=/usr/local/ssl`, which exists nowhere —
  `SSL_CTX_set_default_verify_paths` succeeds while loading **zero** CA
  certificates. Worth measuring on the Linux artifact before calling it
  macOS-only. Its tests also race on a process-global `static TABLE` under
  cargo's parallel threads.
- **rustfmt.** ~239 hunks of pre-existing debt. Restoring a CI check needs a
  formatting pass of its own, which will bury unrelated diffs — do it alone.
