# AsyncPlugin

Blocking work, off the interpreter thread: **one pool, one doorbell, a task
registry.**

Reading a 40 MB file from Pharo freezes every Process in the image for the
length of the read, for the same reason a DNS lookup used to: the primitive
blocks, and a primitive that blocks blocks the GC. `SocketPlugin`'s resolver was
fixed one lookup at a time; this is the general form.

Measured on a live image, the same 40 MB read both ways:

```
40 MB blocking read froze the image for 96 ms
40 MB async read finished in 126 ms while a background Process ran 3,987,078 times
```

## The shape

The only one a Rust plugin may take (`CLAUDE.md`, *The shape every new
capability must take*): **handle, doorbell, collect.** The image submits a job
and gets a SmallInteger handle. A worker thread does the work where the image is
not. One counted signal wakes one pump Process. A later primitive, on the
interpreter thread, copies the bytes out.

```smalltalk
| sema index handle |
sema := Semaphore new.
index := Smalltalk registerExternalObject: sema.
nil asyncStart: 4 signalling: index.

"one pump Process for the whole runtime; nothing else ever waits"
[ | h | [ true ] whileTrue: [
    sema wait.
    [ (h := nil asyncNextCompleted) notNil ] whileTrue: [
        [ self deliver: (nil asyncResult: h) for: h ]
            on: Error
            do: [ :e | nil asyncCancel: h ] ] ] ] fork.

handle := nil asyncReadFile: '/etc/hosts'.
```

| primitive | answers |
|---|---|
| `primitiveAsyncStart: workers signalling: semaIndex` | workers actually started |
| `primitiveAsyncStop` | `true` if the pool stopped, `false` if a job is outstanding |
| `primitiveAsyncReadFile: path` | a task handle |
| `primitiveAsyncWriteFile: path contents: bytes` | a task handle |
| `primitiveAsyncSleep: millis` | a task handle |
| `primitiveAsyncNextCompleted` | the oldest finished handle, **left in place**, or nil |
| `primitiveAsyncState: handle` | 0 pending, 1 done, 2 failed, 3 cancelled |
| `primitiveAsyncResult: handle` | the answer, **retiring the task** |
| `primitiveAsyncError: handle` | errno, or a negative plugin code, or 0 |
| `primitiveAsyncCancel: handle` | `true` if it retired one |
| `primitiveAsyncOutstanding` | tasks the image has not collected |

A `ReadFile` result is a ByteArray, a `WriteFile` result is the byte count, a
`Sleep` result is nil.

## Three rules the image has to keep

**Either collect a handle or cancel it — never neither.**
`primitiveAsyncNextCompleted` *peeks*, so a completion nobody takes stays at the
head of the queue and everything behind it is unreachable. A pump without the
`on: Error do: [ ... asyncCancel: ... ]` arm above spins on the first result it
cannot allocate for, forever. That happened on the first live run. The property
is deliberate — it is what makes a re-run after a scavenge safe — so it is
pinned by a test rather than fixed.

**Once a pump is running, nothing else touches a handle.** Collecting retires
the task, so a second Process asking `primitiveAsyncState:` about the same
handle races the pump and gets a primitive failure when it loses. Also found the
hard way.

**Stop the pool before unloading.** `primitiveAsyncStop` and `shutdownModule`
both refuse while a job is queued or running, because a running job is a worker
executing this library's text and `ioUnloadModule` ends in `dlclose`.

## A plugin cannot allocate a large object

`primitiveAsyncResult:` builds its ByteArray through the proxy's
`instantiateClass:indexableSize:`, which allocates out of what the image has
*already* got and does not grow the heap. The interpreter answers a `NoMemory`
failure by scavenging and re-running the primitive, then by doing a full GC and
re-running it again — and if the object still does not fit, the primitive fails.

Measured on a stock Pharo 12.0 image: **1, 4 and 16 MB reads collect; 32 and
40 MB fail.** It is headroom rather than a constant — 16 MB fails too once the
image is already holding another 16 MB — so a plugin that wants to hand back
something big has to chunk it, and this one does not.

What matters is that the failure is *clean*. Because the drain peeks, a failed
collection leaves the task done, its result intact and its handle still at the
head of the queue. Verified live: state still 1, `asyncNextCompleted` still
answering the same handle, outstanding still 1, and a second attempt failing the
same way rather than finding the result gone. That is `CLAUDE.md` §3 trap 1
working, in the exact failure it was written for.

## Why a thread pool and not tokio

Every job here is *blocking* — a file read, a file write, a sleep. An async
runtime's value is multiplexing many waits onto few threads, and there is
nothing here to multiplex: `std::fs::read` blocks, and under tokio it would go
straight to `spawn_blocking`, which is a thread pool. What tokio would add is a
large dependency tree, a second scheduler inside a VM that already has one, and
runtime-shutdown and cancellation semantics to explain in a plugin whose
contract is meant to fit on a page.

What *would* justify one is genuinely async work — sockets, timers, thousands of
concurrent waits. Sockets already have `SocketPlugin` and the VM's own poll
loop; timers have `AioPlugin`, on that same loop, with no threads at all. The
runtime that was missing is exactly this one: somewhere to put a blocking call.

## One signal per completion, deliberately not coalesced

`rust/examples/ext-sem-soak` measured this path: mean 4.8 µs per signal, and an
image keeping up with 43 million of them over ten minutes without losing one. It
also measured where it breaks — an *unpaced* signaller starves the interpreter
outright. A pool of N threads doing real work cannot get near that, because
every signal here is preceded by a job.

Coalescing on the queue's empty-to-non-empty edge would save a few microseconds
and would put a contract on the image — "drain until nil before waiting again" —
whose violation is a pump asleep with work queued. A hang is a worse failure
than a signal that was not strictly necessary.

## Verification

`cargo test -p async-plugin` runs 9 tests with no VM, executed on
`x86_64-unknown-linux-gnu`: a read landing as bytes and ringing exactly one
doorbell, a write replacing a file and answering its length, a missing file
failing with the platform's own errno, a cancelled job abandoned rather than
answered, completions queueing in the order they finish, a stale handle not
resolving to the next task, the pool refusing to stop while a job is
outstanding and joining every worker when it does, an uncollected completion
blocking everything behind it, and — the `CLAUDE.md` §3 trap 2 property — 1,000
registry reads completing in well under the length of a job that is running at
the time, which is what proves no worker holds the registry lock across its
work.

`aarch64-apple-darwin` is cross-checked with `cargo check --target`, not run.

Live, on a Pharo 12.0 image and a VM built from this tree:

```
pool started with 4 workers, doorbell index 2
40 MB blocking read froze the image for 96 ms
40 MB async read finished in 126 ms while a background Process ran 3987078 times
  collected: state=1, error=0, result='could not collect: PrimitiveFailed'
  and the task survived the failed collection: outstanding = 0
4 MB async read collected 4000000 bytes, state=1
write answered 5 bytes; the file now holds #[1 2 3 4 5]
missing file: state=2 (2 is failed), error=2 (2 is ENOENT), result=nil
the pump collected 4 completions; outstanding = 0
```

## Not verified

* **The `MAX_READ_BYTES` guard** (256 MiB). Nothing has read a file that large;
  the smaller allocation limit above is reached first on a stock image.
* **The primitives' refusals.** The unit tests go through the pool and the
  registry rather than the primitives, because a primitive needs an `&Interp`.
  The live run exercises the happy path of all eleven; a bad worker count, a
  negative sleep, an empty path and a sleep past `MAX_SLEEP_MS` are not driven
  from an image.
* **Descriptor and thread exhaustion.** `primitiveAsyncStart:` answers however
  many workers actually started and `Unsupported` if none did; nothing has
  driven the process into that state to watch it.
* **kqueue platforms, and Windows.** `cmake/rust.cmake` builds this on `UNIX`
  only; nothing here is Windows-aware, and `path_argument` reads a path as bytes
  the way every Unix caller of `sq2uxPath` does.
* **Sustained load.** The soak that proved the doorbell ran against a
  purpose-built harness, not against this pool; nothing has kept a four-worker
  pool saturated for an hour and compared submissions against collections.
