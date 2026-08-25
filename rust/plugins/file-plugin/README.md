# FilePlugin, in Rust

Replaces the VM's core file I/O plugin: the Slang-generated primitive layer
(`smalltalksrc/VMMaker/FilePlugin.class.st` → `FilePlugin.c`, generated at
build time) plus the hand-written unix support files

| C source | lines | ported to |
|---|---:|---|
| `plugins/FilePlugin/src/unix/sqFilePluginBasicPrims.c` | 823 | `src/sqfile.rs` |
| `plugins/FilePlugin/src/unix/sqUnixFile.c` | 384 | `src/dir.rs` |
| `plugins/FilePlugin/src/unix/sqUnixCharConv.c` | 412 | `src/charconv.rs` |
| `plugins/FilePlugin/src/unix/fileUtils.c` | 8 | `src/dir.rs` |

behind the same twenty-six primitives and the same exported C API. The
**Windows** sources (`sqWin32File.h`, the `win/` tree) are not part of this
port and stay C.

## The contract is unchanged

Same module name, same primitive names and accessor depths (taken from the
generated C's `*AccessorDepth` exports, including the two primitives —
`primitiveDirectoryDelimitor`, `primitiveFileStdioHandles` — for which Slang
exports none and the VM reads −1), same argument order, same failure codes,
same explicit stack effects. The primitive bodies mirror the generated C
statement for statement through the raw proxy rather than using the SDK's
typed-argument layer, because the generated C *is* the contract.

The image-side `SQFile` record — a ByteArray holding
`{int sessionID; void *file; char writable, lastOp, lastChar,
isStdioStream;}` — keeps the C compiler's exact layout (24 bytes on 64-bit,
12 on 32-bit), pinned by `tests/layout.rs`. That matters because other
plugins reach into the same bytes: `cmake/plugins.cmake` links
**FileAttributesPlugin** and **UnixOSProcessPlugin** against this library,
consuming `sq2uxPath` / `ux2sqPath` (FileAttributes) and
`sqFileStdioHandlesInto` plus the record layout (UnixOSProcess). All file
I/O goes through C stdio (`libc::fopen` family), never `std::fs`, because
those `FILE *` streams are shared across the plugin boundary.

Exported C surface, verified present in the built `libFilePlugin.so`:

* the whole `sqFile*` family of `FilePlugin.h` (`sqFileOpen`,
  `sqFileOpenNew`, `sqConnectToFile`, `sqConnectToFileDescriptor`,
  `sqFileReadIntoAt`, `sqFileWriteFromAt`, `sqFileAtEnd`, `sqFileClose`,
  `sqFileFlush`, `sqFileSync`, `sqFileTruncate`, `sqFileSize`,
  `sqFileGetPosition`, `sqFileSetPosition`, `sqFileValid`,
  `sqFileDeleteNameSize`, `sqFileRenameOldSizeNewSize`,
  `sqFileStdioHandlesInto`, `sqFileDescriptorType`, `sqFileInit`,
  `sqFileShutdown`, `sqFileThisSession`, `waitForDataonSemaphoreIndex`,
  `signalOnDataArrival`);
* the generated shim's non-primitive exports (`fileValueOf`,
  `fileRecordSize`, `fileOpenNamesizewrite`, `fileOpenNewNamesize`,
  `setMacFileTypeAndCreator`);
* the directory API (`dir_Create`, `dir_Delete`, `dir_Delimitor`,
  `dir_Lookup`, `dir_EntryLookup`, `dir_SetMacFileTypeAndCreator`,
  `dir_GetMacFileTypeAndCreator`, `sqCloseDir`, `sqStdoutToDevTTY`,
  `convertToSqueakTime`);
* the whole of `sqUnixCharConv.h` (`convertChars`, `sq2uxText`, `ux2sqText`,
  `sq2uxPath`, `ux2sqPath`, `sq2uxUTF8`, `ux2sqUTF8`, `ux2sqXWin`,
  `setEncoding`, `setNEncoding`, `setLocaleEncoding`, `freeEncoding`,
  `sqFilenameFromString`, and the seven encoding globals `sqTextEncoding`,
  `uxTextEncoding`, `sqPathEncoding`, `uxPathEncoding`, `uxUTF8Encoding`,
  `uxXWinEncoding`, `localeEncoding`).

`dir_PathToWorkingDir` is declared in `FilePlugin.h` but the unix C never
implemented it; neither does this crate. The C's non-static `int
thisSession` data symbol is internal here — its readers go through
`sqFileThisSession()`.

## What changed underneath

**iconv is gone.** `sqUnixCharConv.c` shelled character conversion out to
`iconv(3)`, keyed by encoding-name strings held in `void *` globals. The
globals, their name-string representation and the
`setEncoding`/`freeEncoding` protocol survive unchanged (unknown names are
still malloc'd and interned), but the conversions themselves are native
Rust for the encodings the VM configures: UTF-8, ISO-8859-1, ISO-8859-15
and MacRoman. The iconv loop's error behaviour is reproduced — invalid or
unconvertible input becomes `?` and is skipped by the leading-ones count of
its first byte; a full output buffer stops the conversion (E2BIG); the
terminator byte is reserved out of `toLen`. A conversion involving any
*other* encoding name falls back to the bounded copy, which is exactly what
the C did when `iconv_open` failed — but the C would have asked iconv first,
so exotic locale encodings (e.g. a `KOI8-R` locale set through
`setLocaleEncoding`) converted in C and only copy here.

**Link-time VM symbols became runtime lookups.** The C plugin resolved
`getVMGMTOffset` (heartbeat.c) and `aioEnable`/`aioHandle`/`aioDisable`
(aio.c) against the core at link time. A Rust cdylib cannot, so they are
fetched once through the proxy's `ioLoadFunctionFrom(name, "")` and cached.
If the core does not export them, `waitForDataonSemaphoreIndex` fails the
primitive cleanly and the GMT offset is 0.

**Undefined behaviour removed** (each behaviour-identical for well-formed
callers):

* `primitiveDirectoryEntry` read the requested-name bytes without checking
  the oop is a bytes object; it now fails the primitive instead.
* `dir_Lookup` with `index <= 0` dereferenced a null `dirent`; it now
  answers "no more entries".
* Directory entry names were converted with a `MAXPATHLEN` (4096) limit
  into 256-byte buffers; the limit is now the buffer's real size
  (`d_name` is at most 255 bytes, so no real name notices).
* Opening a new file probed the Mac type/creator through an *uninitialized*
  buffer before calling two functions that are both no-ops on unix; the
  no-op is now reached without the read.
* `convertCopy` with a zero-length output and a terminator would have
  written before the buffer; sizes are clamped.
* The Slang stored dir-entry results without checking allocation; a failed
  allocation now answers `PrimErrNoMemory` (the same code
  `primitiveFileStdioHandles` always used).
* The C's iconv error-recovery could skip zero bytes and spin forever on a
  low input byte; the skip is at least one.
* `primitiveFileStdioHandles` copied uninitialized struct padding into the
  image; the padding is zeroed.

**Simplifications with no observable difference on Spur:**
`makeDirEntryName:...` wrapped its allocations in `remapOop:` for the
benefit of a moving GC; Spur allocations never move objects (they fail
instead), so the port allocates sequentially. `primitiveFileStdioHandles`
keeps its `pushRemappableOop` dance verbatim, since those proxy entries
exist and cost nothing. The C's `logWarn`/`logError` calls are dropped —
the plugin SDK has no logging channel.

## Verification

`cargo build`, `cargo test` (35 tests), `cargo clippy --all-targets -- -D
warnings` all pass. The exported symbol list was diffed against the C
plugin's. The tests cover:

* **Record layout** — offsets and size against the C struct definition, by
  formula and by the concrete 64-bit numbers.
* **File ops against a real tempdir through libc** — open/write/seek/read
  roundtrip, zero-based buffer offsets, Smalltalk-style `atEnd` (true after
  the last byte is read, position unmoved by the peek), truncate, flush,
  sync, non-truncating write-mode open, create-new collision setting the
  `exists` flag, read-only rejection of writes, rename, delete, fd
  adoption via `sqConnectToFileDescriptor`, session staleness invalidating
  records, descriptor-type answers.
* **Directories** — create/delete, `/` delimiter, full enumeration through
  the one-entry cache, name lookup, missing-directory `BAD_PATH`, the C
  out-parameter ABI, the Squeak epoch offset.
* **Character conversion** — UTF-8 path identity, the C's invalid-sequence
  replacement and skip counts, E2BIG stop, terminator reservation,
  MacRoman↔Latin-15 text (including the euro and the Latin-15 delta),
  CR/LF mapping direction per converter, MacRoman↔UTF-8, the X11 Latin-1
  path, unknown-encoding copy fallback, `sqFilenameFromString`.

## Not verified

No VM runs in the development environment, so an image-side differential
pass should focus on:

* Every primitive's interaction with a live interpreter (stack effects,
  failure codes, the remappable-oop path in `primitiveFileStdioHandles`).
* `ioFilenamefromStringofLengthresolveAliases` — tests exercise the
  no-proxy fallback copy, not the VM's resolver.
* stdio-stream behaviour on a real terminal: the non-blocking `read()`
  path, one-character pushback, `atEnd` on a tty, and
  `primitiveWaitForDataWithSemaphore` / the aio callback.
* Session-ID behaviour across an image save/restart.
* 32-bit images (the layout test's formula covers it, but only the 64-bit
  case has been executed).
* macOS: the crate compiles the iconv-branch semantics everywhere; the
  `__MACH__` CoreFoundation branch's HFS+ NFD/NFC path normalisation is
  **not** implemented, and the `__stdinp` linkage is untested.
* CMake wiring: as with jpeg-plugin, the build still compiles the C
  plugin; switching `cmake/plugins.cmake` over (keeping the
  FileAttributesPlugin/UnixOSProcessPlugin link edges, and CoreFoundation
  no longer needed on macOS) is deliberately a separate change.
