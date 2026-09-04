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
cd rust/plugins && cargo test --workspace --locked          # 611 tests, 50 binaries
cd rust && source ../build/rust-env.sh && cargo test --workspace --locked   # 155 tests
```

**`pharo-vm-sys` refuses to build standalone by design** — it binds headers
whose types depend on the `config.h` CMake generates — so the outer workspace
needs `source build/rust-env.sh` first. Without it you get a build-script panic
that looks like a regression and is not.

No Linux box is available on the usual dev machine; prove Linux with
`cargo check --target aarch64-unknown-linux-gnu --all-targets --locked`
(exclude `squeak-ssl` in the plugins workspace — its vendored OpenSSL will not
cross-compile). Say "cross-checked", never "passes on Linux", in a README.

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

### 1. Async DNS — the smallest complete instance, do this first

`Socket>>lookupName:` freezes every Process in the image inside `getaddrinfo`.
`rust/plugins/socket-plugin/src/resolver.rs:11-13` says so in its own doc: the
lookup blocks and signals the semaphore before returning, *"we're done before
we even started"*. The image-side contract was written for async and the Unix C
never honoured it, so **this needs no image-side change at all**.

Not a five-line diff, because of one self-inflicted obstacle: `start_name_lookup`
holds the `STATE` guard across the whole of `getaddrinfo`, and `resolver_status`
takes the same guard. Spawn without restructuring and the VM freezes on a mutex
instead of on a syscall — same symptom, worse cause. Split into an `AtomicI32`
status plus an `AtomicU32` generation read lock-free, with the mutex only around
the result write:

```
start_name_lookup:  set RESOLVER_BUSY; bump generation; spawn; return
worker:             let r = getaddrinfo(..);
                    if gen == mine { store result }     // lock held for microseconds
                    signal_semaphore(resolver_index)
```

Also: `RESOLVER_BUSY = 2` does not exist in the Rust file, because nothing was
ever busy. `sqResolverAbort` is a no-op; make it a generation bump.

**Prerequisite, and it is the highest-value experiment in this list.** Nothing
in this tree has ever signalled from a genuine foreign thread — socket-plugin's
own SAFETY comment asserts the opposite ("handlers run on the interpreter
thread, so this never races a primitive"), and unix-os-process signals from a
signal handler into a path that takes a `sem_wait`, which is unsound rather
than a model. Yet async DNS, the job pool, the file watcher and everything in
§4 all rest on N background threads hammering that table at rate.

Soak it before building on it, on Linux *and* macOS: a throwaway plugin
spawning 8 threads that signal a contiguous block of registered indices at high
rate for an hour against a GC-heavy image, asserting (a) no lost signals —
counted requests against image-side receipts per index, which the counting table
should make exact; (b) the `requestMutex` contention profile against the VM
thread's own `doSignalExternalSemaphores`; (c) that `sigprocmask` — not
`pthread_sigmask` — behaves in a multithreaded process on both platforms, which
POSIX leaves unspecified and glibc only gets right by accident; (d) the wake
latency distribution, to confirm rather than assert "worst case one relinquish
quantum". A week, and cheap. If it does not hold, every ambitious item below
needs a different wakeup edge.

### 2. Hot-reloading Rust plugins — best value-to-effort in the list

Primitive 571 `Smalltalk unloadModule:` already runs `shutdownModule`,
broadcasts `moduleUnloaded` to every other module, `dlclose`s, removes the
entry, then flushes external primitives and forces an interrupt check. It was
never worth using because rebuilding a C plugin meant a full VM build. A Rust
crate rebuilds in seconds.

```
cargo build -p large-integers --release && install -m755 <lib> build/vm/Plugins/
```
```smalltalk
Smalltalk unloadModule: 'LargeIntegers'.
1000 factorial printString size   "re-dlopens on next call: new code, same image"
```

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

Use `install`, never `cp` — writing into a mapped inode is SIGBUS. And verify
empirically that `dlclose` actually unmaps: glibc keeps a DSO mapped on a
non-zero `l_tls_dtor_count`, in which case the reload silently keeps running
old code. Test with a version-stamping primitive.

This is also the only path in the tree that ever runs a shutdown hook, so it is
the first thing to give shutdown code any coverage — which cuts both ways.

### 3. fd watches and timers, without the reactor rewrite

`primitiveAioWaitFd:events:signalling:` and `primitiveAioTimerAfter:signalling:`
need nothing from a `mio` port. `aioEnable`/`aioHandle`/`aioDisable` are already
resolvable from any plugin (`rust/plugins/socket-plugin/src/aio.rs:55-57`
declares them as undefined externs), handlers already run on the VM thread, and
`aioEnable` already takes `AIO_EXT` precisely so it will *not* force
`O_NONBLOCK|O_ASYNC` on a descriptor the image owns. A timerfd registered the
same way gives OS-resolution timers today. ~150 lines, three platforms, no ABI
motion. Handlers are one-shot by contract — the mask is cleared before dispatch
— so re-arm inside the handler.

The reactor rewrite (a persistent `mio::Poll` replacing the
`epoll_create1`/N×`epoll_ctl`/`epoll_wait`/`close` **per poll**) is a separate,
later, Linux-first wave: it touches `libPharoVMCore`, three platform files and
the heartbeat poll handshake. Do not let it hold this item hostage.

### 4. AsyncPlugin — one runtime, one doorbell, a task registry

A rust-only cdylib (`add_rust_only_plugin`, so it is invisible to
`abi-check.sh`) holding a runtime, `Registry<Arc<Task>>`, a queue of completed
handles, and **one** external-semaphore index for the whole runtime. The image
runs one pump Process and never sees a semaphore.

Three traps, all learned the hard way and all generalising beyond this item:

1. **The drain must be peek-and-ack, not pop.** `IntoReturn` runs *after* the
   body, and an allocation failure makes the interpreter scavenge and **re-run**
   the primitive. Pop-then-allocate loses completions permanently on the retry,
   with no error anywhere. Rule: allocate first, mutate Rust state last, never
   answer `NoMemory` after a side effect.
2. **`Registry::with` holds the mutex across the closure.** A worker reaching
   its task through the registry stops the interpreter dead. Use
   `Registry<Arc<Task>>`, clone out, drop the guard.
3. **`ioUnloadModule` is image-reachable** and `dlclose`s a library whose code
   a worker is executing. `shutdownModule` must refuse while any job is
   outstanding — see §2.

### 5. "Run the VM off the main thread" — mostly already true

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
