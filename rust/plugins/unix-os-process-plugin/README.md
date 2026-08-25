# UnixOSProcessPlugin, in Rust

Replaces the Slang-generated `plugins/UnixOSProcessPlugin/src/common/UnixOSProcessPlugin.c`
(D. T. Lewis's OSProcess plugin, version 4.6.4 Cog,
`VMConstruction-Plugins-OSProcessPlugin.oscog-dtl.66`) — fork/exec of child
processes, pipes, standard-stream handles, environment access, signal
forwarding to Smalltalk semaphores, process/group/session ids, `fcntl` file
locking, and the fd-level utilities around them.

## The contract is unchanged

Same module name string (version stamp and `(e)` suffix included — the VM
compares the prefix, the image reads the rest), all **94 primitives** under
their exact names, same argument shapes, same failure behaviour, same
accessor-depth bytes (verified mechanically against the C's
`...AccessorDepth` exports), and the same non-primitive exports: `forkSqueak`,
`getCurrentWorkingDirectoryAsType`, `moduleUnloaded`, plus the standard
`getModuleName` / `setInterpreter` / `initialiseModule` / `shutdownModule`.

## What changed underneath

**Cross-module symbols resolve at runtime.** The C linked directly against VM
core and FilePlugin symbols. A stand-alone cdylib cannot, so each is obtained
once through the proxy's `ioLoadFunctionFrom` and cached:

| symbol | module tried | used by |
|---|---|---|
| `ioGetEnvVec` | `""` (the VM) | environment vector (first choice, as in C) |
| `getProcessEnvironmentVector` | `""` | environment vector fallback |
| `getProcessArgumentCount` / `getProcessArgumentVector` | `""` | `primitiveArgumentAt[AsBytes]` |
| `sqFileStdioHandlesInto` | `"FilePlugin"`, then `""` | `primitiveGetStd{In,Out,Err}Handle` |
| `GetAttributeString` | `"os_exports"` | JIT detection for the sigaltstack decision |

When no VM is present at all (unit tests), the environment falls back to the
live process environment; the argv primitives fail with `Unsupported`.

**`vfork()` became `fork()`.** Rust cannot soundly host a returns-twice call.
Under `vfork` the child shared the parent's memory and the parent was
suspended until exec, which the C exploited: a child-side validation failure
(`chdir` failed, bad argv/env buffer) called `primitiveFailFor` in *shared*
memory and so failed the parent's primitive. With `fork` that channel is
gone, so:

* everything checkable is validated **in the parent before forking** (bad
  buffers, bad working-directory/executable strings fail the primitive
  without creating a child — the C created a child that immediately
  `_exit(-1)`ed);
* a `chdir` failure inside the child can no longer fail the primitive: the
  child writes a message to stderr and exits with status 255, exactly the
  signature an exec failure always had (in both versions);
* the parent resumes immediately instead of waiting for the exec.

The argv/env pointer fixing now runs in the parent, mutating the image's
flattened buffers — which is what effectively happened under `vfork` anyway.
The child-side path is a straight line of async-signal-safe calls: `chdir`,
`fflush`/`dup2`/`rewind`, the close-everything loop, handler restoration
(atomic reads + `sigaction`), `execve`, `_exit`.

**Signal handler bodies mirror the C, including its de-facto-safe parts.**
`signalSemaphoreWithIndex` is called from the handler exactly as every
Squeak/Pharo VM has always done — it is not strictly async-signal-safe by
POSIX's list, but the VM's implementation is written for this use; flagged
here rather than redesigned. Likewise kept: the handler's no-op
re-registration call (`forwardSignal:toSemaphoreAt:` invoked from inside the
handler, which immediately answers `SIG_ERR` because the signal is already
registered), and the C's `char semaIndex` read — a semaphore index above 127
is read back negative and never signalled, and registration truncates the
index to 8 bits, because the C's `unsigned char semaIndices[]` did. The
registration arrays are atomics (`static mut` reads from a handler are a data
race in Rust); atomic loads are async-signal-safe.

**No in-image scratch objects.** The C allocated throw-away Smalltalk objects
as C workspace: a String for `transientCStringFromString`, a 1024-byte String
for `realpath` to write into (overrunnable — realpath with `PATH_MAX > 1024`
was a heap overflow in image memory), a growing String ladder for `getcwd`, a
`struct stat`-sized ByteArray. These are Rust-side buffers now
(`realpath(path, NULL)` lets libc size the result; the C's `len >= 1024 →
fail` check is kept so answers agree). Observable behaviour is unchanged;
garbage-collector pressure is slightly lower.

**Out-of-bounds accesses are gone.**

* `primitiveKillOnExit` copied `pidCount + 1` elements out of a
  `pidCount`-element Array into a `malloc(pidCount)` buffer — a one-element
  heap overflow plus a one-slot over-read of image memory on every call. The
  port copies exactly `pidCount` (the extra element was never used).
* `fixPointersInArrayOfStrings` read the NULL-terminator slot without
  checking it fit inside the buffer. The port requires the slot to fit and
  fails with `BadArgument` otherwise (the image always leaves room for it).
* `semaIndices[sigNum]` was indexed with whatever integer the image sent;
  out-of-range signal numbers now fail cleanly instead.
* `setSignalNumber:handler:` returned the **uninitialized**
  `oldHandlerAction.sa_sigaction` when `sigaction` failed; the port answers
  `SIG_ERR`, which is what every caller tests for.
* A null `FILE*` inside an otherwise-valid SQFile record answers fd −1 /
  fails instead of crashing in `fileno`; unchecked `fdopen` results in the
  pipe primitives fail the primitive instead of storing a null stream.

**Garbage stack values fail cleanly.** Where the C read
`stackIntegerValue(...)` without checking and marched on with 0 (leaving the
failure flag set while still pushing an answer — e.g. `primitiveArgumentAt`
with a non-integer index), the port fails the primitive. Every primitive also
checks its argument count first (the C only did for the fork/exec pair);
with the wrong arity the C corrupted the stack.

**Small mechanical notes.** The SIGCHLD `sigaction` sets `SA_SIGINFO`, since
the handler really has the three-argument signature the C assigned without
declaring (behaviourally identical). Diagnostics go to stderr instead of the
VM's `logError` machinery; the child after fork uses bare `write(2)`.
`errno`-answering primitives (`primitiveChdir`, the `stat` pair) and
`primitiveNice`'s clear-errno-then-test dance are as in the C.

## Oddities kept on purpose

* `primitiveArgumentAt` / `primitiveEnvironmentAt` answer strings **one byte
  longer** than the C string, trailing NUL included — the C really did
  allocate `len + 1`. The by-key variants (`...AtSymbol`), `getcwd`,
  `realpath`, `strerror` and the version/module strings are exact-length.
* `primitiveCreatePipe` answers `{reader. writer}` — reader at index 1 (the
  C's remappable-oop push/pop order), and does *not* touch the SIGPIPE
  handler; `primitiveMakePipe` sets SIGPIPE to `SIG_IGN` first. Both kept.
* `primitiveSQFileFlushWithSessionIdentifier` pops its session argument but
  never compares it (the C validated against the interpreter's own session
  identifier, same as the plain variant). The SetBlocking / SetNonBlocking /
  SetUnbuffered `WithSessionIdentifier` variants *do* compare. All kept.
* A non-integer signum for `primitiveKillOnExit` empties the pid list but
  still succeeds — the C's failure path amounted to exactly that.
* `initialiseModule` registers the `atexit` hook that signals the registered
  pids (`SIGTERM` unless the image chose otherwise); `shutdownModule`
  restores replaced signal handlers but leaves the registration table, as the
  C did.

## Not ported (dead code in the C)

The generated C carried never-called static helpers: the SQSocket accessors
(`socketValueOf`, `socketDescriptorFrom`, `isSQSocketObject`, ... — no
exported primitive uses sockets), `pointerFrom:`, `sigHoldNumber` and
friends, `setSigIntDefaultHandler`/`setSigIntIgnore`. Nothing references
them; they have no Rust counterpart. Consequently this port needs nothing
from SocketPlugin at all — the C's `#include "SocketPlugin.h"` fed only dead
code.

## Verification

`cargo build`, `cargo test` (19 tests), `cargo clippy --all-targets -D
warnings` all pass. The exported surface was diffed mechanically against the
C: all 94 primitive symbols identical, all accessor-depth bytes equal to the
C's exports (−1 for the ones the C left implicit), module exports present,
`getModuleName` byte-identical.

What the tests cover without a VM:

* **SQFile layout** pinned against `FilePlugin.h` (size, field offsets,
  session round-trip, zero padding in serialized records).
* **Pointer fixing** on image-format flattened buffers: correct `char *[]`
  construction, NULL termination, and each rejection case (offset out of
  range, oversized slot table, missing/dirty terminator).
* **fork/exec end to end**: `/bin/echo` spawned through the same
  flattened-buffer → `fix_pointers` → `fork_and_exec` path the primitive
  uses, stdout dup'ed onto a pipe and read back; a missing executable exits
  255 (the C's signature).
* **Pipes**: create/write/flush/read/close round-trip through the `fdopen`ed
  streams.
* **Signals**: the registration state machine (register, double-register
  refused, unregister, double-unregister refused), a real raised `SIGUSR1`
  reaching the stubbed semaphore call with the right index, the SIGCHLD
  reaper doing the same, and out-of-range signal numbers refused.
* **Kill-on-exit**: a paused child actually killed through the atexit hook's
  code path and reaped.
* Environment vector fallback, errno helpers, protection-mask digit
  arithmetic, module-name agreement.

## Not verified

No runnable VM exists in this environment, so everything that needs a live
interpreter is untested and should lead the image-side differential pass:

* All proxy plumbing: stack access, object allocation, `stObjectat:put:`,
  the failure codes as seen by image fallback code.
* Interop with **real FilePlugin records** (pipe handles fed to FilePlugin
  reads/writes, `sqFileStdioHandlesInto` via `ioLoadFunctionFrom`) and the
  runtime resolution of every symbol in the table above.
* `primitiveForkSqueak` — a forked child continuing to run the image.
* The sigaltstack path (needs a JIT VM answering attribute 1008) — only the
  "no JIT → plain `signal()`" path runs in tests.
* Signal forwarding from a non-VM thread (the mask-and-resend path).
* File locking against another process actually holding locks.
* macOS build (the `__stdinp`/`__error` cfgs), and 32-bit images. Platforms
  other than Linux/macOS would need small cfg additions (`NSIG`, the errno
  location, the stdio globals).

## Not done yet

**CMake wiring.** The crate builds and exports the right surface, but the
build still compiles the C plugin. Switching over means adding this crate to
`cmake/rust.cmake` and dropping `UnixOSProcessPlugin` from the plugin list —
deliberately left as a separate change so the swap is reviewed on its own.
