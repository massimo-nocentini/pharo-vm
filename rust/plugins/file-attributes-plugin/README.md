# FileAttributesPlugin, in Rust

Replaces the Slang-generated `FileAttributesPlugin.c` (~1,200 lines), the
hand-written `faCommon.c`, and the **Unix** support layer `faSupport.c` with
one crate behind the same sixteen primitives. This is the plugin the image's
`FileSystem` layer uses for `stat`/`lstat` attributes, access checks, symlink
targets, path-encoding conversion and directory enumeration.

**Windows keeps its C.** The Windows support layer (`src/win/faSupport.c`,
with its vendored `dirent.h` and wide-character path handling) is not ported;
this crate compiles to an empty library off Unix (`#![cfg(unix)]`), and the
Windows build must keep using the C plugin.

## The contract is unchanged

Same primitive names, same argument shapes, same answers, and — because the
image maps them to exceptions — exactly the status codes of `faConstants.h`
(`src/codes.rs` is a line-for-line copy, with a pinning test). Accessor
depths are the ones the C exports; the four primitives the C leaves without
an `AccessorDepth` symbol (masks, logical drives, path max, version) export
`-1` here, the value the VM infers from the missing symbol.

`getModuleName` answers `FileAttributesPlugin`; the C answers
`"FileAttributesPlugin FileAttributesPlugin.oscog-akg.49 (e)"`, and the VM
compares only the module-name prefix (`callInitializersIn`). The image reads
the plugin version through `primitiveVersionString`, which answers `2.0.8` as
the C does.

## What changed underneath

**`std::fs` replaces the raw syscalls wherever it loses nothing.**
`MetadataExt` exposes every `struct stat` field this plugin answers as the
raw integer the OS reported, so `stat`/`lstat` go through
`std::fs::metadata`/`symlink_metadata` with no fidelity cost and no
`MaybeUninit<libc::stat>`. `readlink`, `chmod`, `chown` and `lchown` likewise
become `std::fs::read_link`, `set_permissions` and
`std::os::unix::fs::{chown, lchown}` -- each carrying its errno in the error
value instead of leaving it in a global to be read back.

Directory walks are `std::fs::ReadDir` rather than a raw `*mut DIR`. That
removed the `unsafe impl Send`, the manual `Drop`, and the hand-rolled
`errno_location()` -- which had `cfg` arms for Linux and the BSDs only, and so
would not have compiled on Solaris or NetBSD. The C cleared `errno` before its
`readdir` loop purely to tell end-of-stream from failure; `Iterator::next`
answering `None` versus `Some(Err)` *is* that distinction, checked by the
compiler rather than by convention. `read_dir` also skips `.` and `..` itself,
so the explicit filter is gone.

`libc` remains for the three things std has no equivalent for: `access(2)`
(std can test existence, not R_OK/W_OK/X_OK against the real uid), the
`tm_gmtoff` of `localtime_r` (std has no timezone support), and the `S_IF*`
masks.

**No raw pointer in image memory.** `primitiveOpendir` hands the image a
ByteArray shaped like the C `FAPathPtr` — `{ int sessionId; fapath *ptr; }`,
16 bytes on 64-bit — and the walk primitives trust what comes back. In the C
that trust is misplaced twice over: the generated `initialiseModule` never
calls `faInitialiseModule`, so `vmSessionId` stays 0 forever and the
"session id check" accepts the 0 that `faInvalidateSessionId` writes into
closed handles; and the pointer is then dereferenced as-is, so a closed,
stale or fabricated handle is a use-after-free. This port keeps the byte
layout, the size check (`PrimErrBadArgument`) and the session-id comparison
(against the same never-initialised 0), but the pointer slot carries a key
into a process-local registry; an unknown key fails with
`FA_BAD_SESSION_ID` (−17), the error the design intended for those handles.

**FilePlugin is a runtime lookup, not a link dependency.** The C plugin is
linked against FilePlugin (`target_link_libraries(FileAttributesPlugin
PRIVATE FilePlugin)`) for exactly two symbols: `sq2uxPath` and `ux2sqPath`
from `sqUnixCharConv.c` (image encoding ↔ platform encoding). This port
fetches both through `ioLoadFunctionFrom("sq2uxPath", "FilePlugin")` at first
use and caches them, so the two plugins keep sharing one conversion state.
When FilePlugin is absent, a built-in fallback reproduces what those
functions do on Linux — where `setLocaleEncoding` is never called and both
path encodings stay UTF-8 — including `convertChars`' peculiar error
handling: an invalid sequence is skipped by the leading-1-bit count of its
first byte (swallowing valid bytes after a bad lead) and replaced by one `?`,
and a full output buffer truncates silently. Unit tests pin those vectors.

**Alloca and fixed buffers are gone.** Paths live in checked `Vec`s that
enforce the same `PATH_MAX` comparisons the C makes (same off-by-including
`>=`, same `FA_STRING_TOO_LONG`), and the walk primitives no longer
`alloca`/`memcpy` the handle on every call.

## Divergences (all invisible to a correct image, most to a broken one)

| C behaviour | here |
|---|---|
| Stale/closed/garbage directory handle: pointer dereferenced (UB) | fails `FA_BAD_SESSION_ID` |
| Handle validated with `stSizeOf`/`arrayValueOf` on any object — a 2-slot Array's oops get reinterpreted as struct bytes | non-bytes handle fails `PrimErrBadArgument` |
| `faSetStDir` of an empty name reads one byte *before* the buffer | takes the append branch: empty dir name means `/` |
| `readlink` target filling the buffer exactly: NUL written one past the end | length-checked; over-long target still fails `FA_STRING_TOO_LONG` |
| `localtime()` result dereferenced unchecked (crash on unrepresentable time) | zero timezone offset for such times |
| Failed `Array` allocation mid-`attributeArray:for:mask:`: continues into `storePointer` on oop 0 | fails immediately with the same final code (−15 / `PrimErrNoMemory`) |
| `primitiveOpendir` error paths leak the `fapath` and sometimes the `DIR` | dropped/closed |
| Arity never checked; wrong-arity sends read stack garbage | wrong arity fails `PrimErrBadNumArgs` |
| `primitiveRewinddir` on a directory that became empty answers the previous entry from the stale buffer | **kept**, quirk and all |
| `st_nlink` boxed 32-bit in the attribute array but 64-bit as single attribute 5 | **kept**, pinned by a test |

## Verification

`cargo test -p file-attributes-plugin`: 42 tests.

* **Path encoding**: identity for valid UTF-8, the `?`-replacement skip
  vectors (`C3 28` → `?`, `E2 82 41` → `?B`, `FE`/`FF`, bare continuation,
  truncated tail, overlong, surrogate), full-buffer truncation semantics.
* **`fapath` bookkeeping**: separator handling, the exact length limits,
  directory/file composition across entries, embedded-NUL (`strlen`)
  semantics, plat↔st round-trips.
* **Attributes** (against files created in a scratch directory, with
  `std::fs::Metadata` as an independent witness): field extraction, the
  directory-size-is-zero rule, the 13-slot boxing table and the
  single-attribute table, ENOENT and EACCES → `FA_CANT_STAT_PATH`, symlink
  vs target via `lstat`/`stat`, dangling symlinks, `readlink` targets,
  `access()` results, the Squeak epoch offset (2,177,452,800) and timezone
  formula, the `S_IF*` mask table.
* **Directory sessions**: full walk with `.`/`..` skipped, empty directory,
  missing directory, rewind, double close, registry lifecycle, and the
  handle's size/offsets against the C struct layout.

## Not verified

* **Image-side differential testing.** No VM runs in the port environment,
  so nothing above exercises a real interpreter: proxy boxing calls
  (`positive32/64BitIntegerFor`, `signed64BitIntegerFor`,
  `storePointerofObjectwithValue`, `primitiveFailForOSError`), oop traffic,
  and the exact failure codes as the image sees them all need the
  differential pass (`FileSystemTests`, `FileAttributesPluginPrims` users).
* **The `ioLoadFunctionFrom` path.** Resolution of `sq2uxPath`/`ux2sqPath`
  from a live FilePlugin is untested here; only the fallback conversion is.
  In particular, on **macOS** FilePlugin normalises paths (decomposed UTF-8
  for HFS+) and the fallback does not — with FilePlugin present behaviour
  matches the C, without it non-ASCII paths on macOS would diverge.
* `chown`/`lchown` success paths (need root), `closedir` failure (−12), and
  `readdir` failure (−16).

## Not done yet

* **CMake wiring.** The crate builds and exports the exact C symbol surface,
  but the build still compiles the C plugin; switching
  `cmake/plugins.cmake` over (and dropping the `FilePlugin` link line) is a
  separate, reviewable change.
* **Windows**, deliberately — see above.
