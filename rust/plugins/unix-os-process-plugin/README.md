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

## Divergences

Places where the port knowingly does something the C does not.

* **The Rust installs an alternate signal stack the C does not.** In
  `needSigaltstack` the C reused the same `stack_t` it had just filled with
  `sigaltstack(0, &sigstack)`, assigning only `ss_size` and `ss_sp`
  (`UnixOSProcessPlugin.c:1446`) and leaving `ss_flags` alone. That line is
  only reached when the query answered `ss_size == 0` or `ss_flags &
  SS_DISABLE`, and a process with no alternate stack gets exactly `ss_flags =
  SS_DISABLE` back on both platforms (4 on Darwin, 2 on Linux) — so the C fed
  `SS_DISABLE` straight back into the install. The C therefore *disables* the
  alternate stack, leaks the buffer it just malloc'd, answers 1 anyway, and
  goes on installing `SA_ONSTACK` handlers with no alternate stack to run on,
  which the kernel treats as "use the normal stack".

  **This port does not copy that.** `install_request` in `signals.rs` asks for
  `ss_flags = 0` and a real 128 KiB (64-bit build) alternate stack, and a test
  pins that it does — so on both Linux and macOS the Rust plugin has an
  alternate stack where the C build has none. Matching the C's bug instead is
  an open question, not a settled one: it would change the JIT's signal
  delivery on Linux as much as on macOS, and nothing in this crate's tests can
  exercise the path (it needs a JIT VM answering attribute 1008). It belongs
  in its own commit, with a JIT-VM differential run behind it — not folded
  into a port.

* On `sigaction` failure `setSignalNumber:handler:` answers `SIG_ERR` where
  the C answered the uninitialized `oldHandlerAction.sa_sigaction`; the
  out-of-bounds accesses and unchecked stack reads listed under *What changed
  underneath* fail the primitive instead. Those are the other places the Rust
  is deliberately not bug-compatible.

## Not ported (dead code in the C)

The generated C carried never-called static helpers: the SQSocket accessors
(`socketValueOf`, `socketDescriptorFrom`, `isSQSocketObject`, ... — no
exported primitive uses sockets), `pointerFrom:`, `sigHoldNumber` and
friends, `setSigIntDefaultHandler`/`setSigIntIgnore`. Nothing references
them; they have no Rust counterpart. Consequently this port needs nothing
from SocketPlugin at all — the C's `#include "SocketPlugin.h"` fed only dead
code.

## macOS

The C had **no Darwin branch at all**: `grep -ril 'apple\|darwin\|__MACH__'`
over `plugins/UnixOSProcessPlugin/` finds nothing, and apart from the
`SQUEAK_BUILTIN_PLUGIN` switches the only conditionals in its 5050 lines are
an `__OpenBSD__` include block (`:28`) and four feature tests:
`isIntegerObject` (`:310`, guarding an extern declaration), `SA_DISABLE`
(`:1434`, the sigaltstack arm — neither glibc nor Darwin defines it, so both
take the `SS_DISABLE` `#else`), `SA_NOCLDSTOP` (`:4244`, `:4396`, `:4414`) and
`SIG_HOLD` (`:4590`). There is no macOS specification to reproduce here — the
C simply let each platform's headers decide, which is the one thing Rust
cannot do. Three spots therefore need an arm the C did not, and a `compile_error!` in `lib.rs` names them so a third Unix fails with
a sentence rather than three unresolved imports:

| spot | Linux | Darwin |
|---|---|---|
| errno location | `__errno_location()` | `__error()` |
| `FILE *stdin/stdout/stderr` | `stdin` … | `__stdinp` … |
| `NSIG` | 65 (real-time signals) | 32 (no `SIGRTMIN`) |

Everything else that differs is a *value*, read from `libc` rather than
hardcoded. The per-OS ones are pinned by tests; `MINSIGSTKSZ` deliberately is
not, because it varies by architecture rather than by OS and the code depends
only on the C's requested size winning the `max` against it:

| constant | Linux | Darwin |
|---|---|---|
| `F_RDLCK` / `F_UNLCK` / `F_WRLCK` | `int` 0 / 2 / 1 | `short` 1 / 2 / 3 |
| `SS_DISABLE` | 2 | 4 |
| `MINSIGSTKSZ` | *per architecture, not per OS*: 2048 on x86-64 glibc, 5120 on aarch64 glibc, 4096 on glibc ppc64/s390x, 6144 on musl aarch64 | 32768 |
| `getdtablesize()` | `RLIMIT_NOFILE`, 1024 by default | same, clamped to `kern.maxfilesperproc` — 245760 here |

Two of those are worth carrying into the image-side differential pass:

* **Lock-type numbers reach Smalltalk.** `primitiveTestLockableFileRegion`
  answers the conflicting lock's raw `l_type` in slot 3, so the same
  conflicting write lock reports **3** on a Mac and **1** on Linux. The C
  answered the platform's own constants in exactly the same way, so this is
  faithful, not a port bug — but any OSProcess image code comparing slot 3
  against a hardcoded number will read differently on Darwin. (The `lockable`
  boolean in slot 1 is computed against each platform's own `F_UNLCK` and is
  correct on both.) The `l_type` field is a `short` on both platforms while
  the constants are `int` on glibc, which is what stopped this crate
  compiling on a Mac; the conversion now happens once, in named constants,
  with a `const` block proving it lossless.
* **The pre-exec close loop is ~250× longer on a Mac.** `fork_and_exec`
  reproduces the C's `for (fd = 3; fd <= getdtablesize() - 1; fd++) close(fd)`
  byte for byte. Both platforms answer the `RLIMIT_NOFILE` soft limit there,
  but the defaults are worlds apart: 1024 on a stock Linux against 245760 on
  this Mac (Darwin clamps to `kern.maxfilesperproc`; `ulimit -n` is 1048576),
  measured at about 47 ms of `close(2)` per spawned child. Darwin
  has neither `closefrom(3)` nor `close_range(2)`, and `proc_pidinfo` would
  enumerate a different set, so there is no faithful shortcut and the loop
  stays. It is if anything cheaper than the C's: under `vfork` the parent was
  suspended for the whole sweep, under `fork` it is already running.

Nothing else needed a branch. `struct flock` orders its fields differently on
the two platforms (`l_start, l_len, l_pid, l_type, l_whence` vs `l_type,
l_whence, l_start, l_len, l_pid`) but every access is by name; `sigset_t` is
4 bytes on Darwin against 128 on glibc but is only ever passed by pointer;
`libc::stat` carries the `stat$INODE64` link name where x86-64 macOS needs
it; `wait(2)` status encoding, `setsid`/`setpgid`, and `signal()`'s BSD
semantics are the same on both.

## Verification

**On aarch64 macOS**, where this wave was done: `cargo build`, `cargo test`
(24 tests) and `cargo clippy --all-targets -- -D warnings` all pass.

**For Linux, only cross-checks were run**, from that same Mac:
`cargo check -p unix-os-process-plugin --target aarch64-unknown-linux-gnu
--all-targets` and the same invocation of `cargo clippy` with
`-- -D warnings` are clean. The test suite has **not** been executed on Linux
in this wave — no Linux binary has been run at all — so read the Linux side as
"compiles and lints clean", not "tested". What keeps that honest rather than
merely hopeful is that every platform difference in this crate is behind a
`cfg` or read from `libc`; nothing changes Linux behaviour unconditionally.

The exported surface was diffed mechanically against the C: all 94 primitive
symbols identical, all accessor-depth bytes equal to the C's exports (−1 for
the ones the C left implicit), module exports present, `getModuleName`
byte-identical.

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
* **Platform constants**: that `flock`'s `l_type`/`l_whence` really are
  `c_short` here (a type ascription, so it fails at build time), that
  narrowing the `F_*LCK` constants into them is lossless (a `const` block, so
  it fails at build time too), that the numbers are this platform's own, and
  a real `F_SETLK` → child `F_GETLK` → `F_UNLCK` round trip proving the
  `l_type` a blocking lock reports is the one slot 3 will answer. Likewise
  `SS_DISABLE` and `NSIG` per platform, that the C's requested sigaltstack
  size clears `MINSIGSTKSZ` whatever the architecture makes it, and that
  `needSigaltstack`'s install request asks for `ss_flags = 0` — the divergence
  above — rather than handing `SS_DISABLE` back.

Tests that fork or change a signal disposition now share one lock
(`signals::SIGNAL_TEST_LOCK`): the SIGCHLD handler one test installs has no
`SA_RESTART` — the C's flags, kept — so another test's blocking `waitpid`
would return `EINTR` the moment its child exits, and `note_vm_thread` resets
the sigaltstack decision under everyone's feet. A latent flake on Linux as
much as on macOS; closed while the file was open.

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
* **The test suite on Linux.** This wave ran it only on aarch64 macOS; for
  Linux it ran `cargo check` and `cargo clippy` against
  `aarch64-unknown-linux-gnu` from the same Mac. Executing the 24 tests on a
  real Linux host is the cheapest thing still outstanding.
* 32-bit images. (The macOS build is no longer on this list: the crate builds,
  links and passes its whole suite on aarch64 macOS, `__stdinp`/`__error`
  included. What is still untested there is everything above that needs a live
  VM — the same list as on Linux. Platforms other than Linux and Apple are now
  refused by a `compile_error!` rather than failing as unresolved imports.)

## Not done yet

**CMake wiring.** The crate builds and exports the right surface, but the
build still compiles the C plugin. Switching over means adding this crate to
`cmake/rust.cmake` and dropping `UnixOSProcessPlugin` from the plugin list —
deliberately left as a separate change so the swap is reviewed on its own.
