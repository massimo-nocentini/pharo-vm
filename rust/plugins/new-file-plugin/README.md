# NewFilePlugin, in Rust

Replaces the Unix side of the fd-based file plugin: the Slang-generated
primitive shims (whose source of truth is
`smalltalksrc/VMMaker/NewFilePlugin.class.st` — the generated C is not
checked into this repository) and the hand-written
`plugins/NewFilePlugin/src/unix/UnixFile.c` behind them, ~306 lines of
open/read/write/lseek/mmap plumbing.

**Windows keeps its C** (`plugins/NewFilePlugin/src/win/Win32File.c`); this
crate `compile_error!`s anywhere but Unix rather than half-working.

## The contract is unchanged

Same module name, same twenty primitives, same argument order, same answers,
same failure codes on the paths the C had. The handle the image holds — an
`ExternalAddress` whose one machine word is a raw `NewFile_t*` /
`NewDirectory_t*` (or, for `primitiveDirectoryNext`, a `char*` into
`readdir(3)`'s storage) — is written and read exactly as the C's
`pointerAtPointer:` did, so it is byte-compatible: an image could swap
plugins mid-session and its open handles would still parse. The `NewFile`
record itself even keeps the C struct's field-for-field layout, although
nothing outside the plugin is supposed to look inside it.

The whole `NewFile_*` / `NewDirectory_*` API from
`plugins/NewFilePlugin/include/common/NewFile.h` is exported `extern "C"`
under its C names (see [`src/newfile.rs`](src/newfile.rs)), so any other
code that resolved those `PHARO_NEWFILE_EXPORT` symbols finds them
unchanged. Syscall use is the C's exactly: the same `open(2)` flag mapping
per mode/disposition/append flag, creation mode 0644, `mkdir` mode 0755,
`MAP_SHARED` mappings, and the same error conventions (`NULL`, `-1`, `0` or
`false`, each where the C used it).

### Oddities kept deliberately

* **`NewFile_memoryUnmap`'s count check is inverted** in the C: the unmap
  that drops the count to zero returns *before* `munmap(2)` (the last
  mapping is never released), while an unmap that still leaves references
  outstanding unmaps immediately, dangling them. Reproduced exactly:
  "fixing" it would make pointers that stay valid under the C dangle under
  Rust and vice versa, which is precisely what differential testing must not
  see. Flagged here so it can be fixed in both implementations at once.
* `NewFile_memoryMap` takes the length from `NewFile_getSize` at first map
  time; a failed size (−1) becomes `SIZE_MAX`, the `mmap` fails, NULL is
  answered. An unknown protection maps `PROT_NONE`; a failed `mmap` leaves
  `MAP_FAILED` parked in the handle (harmless — the count stays 0).
* `NewFile_tell` answers 0 for a NULL handle where every other query
  answers −1.
* An unknown creation disposition adds no flags at all (acts like
  `OpenExisting`); an unknown seek mode is a no-op; `NewFile_seek` and
  `NewDirectory_rewind` discard their syscalls' results.

## What changed underneath

* **`primitiveDirectoryOpen` no longer reads past the path.** The C built a
  NUL-terminated copy of the path and then passed the *original,
  unterminated* pointer to `opendir(3)`, which scans until it happens upon a
  zero byte — undefined behaviour, and potentially the wrong directory. The
  terminated copy is used.
* **Read/write buffers are bounds-checked.** The C handed
  `firstIndexableField(bufferOop) + bufferOffset` straight to
  `read(2)`/`write(2)` with no type or bounds check: a short buffer meant
  heap corruption in image memory. The four read/write primitives now
  require a word- or byte-indexable buffer, a span inside it, and — for the
  destination of a read — mutability, failing cleanly otherwise
  (`BadArgument`/`BadIndex`/`NoModification`). In-bounds calls are
  unchanged.
* **`primitiveFileOpen` fails before its side effect.** The generated shim
  converted flags/disposition/mode, ignored the conversion failure flag, and
  opened the file anyway — a *failing* primitive could still create a file
  on disk and leak the handle. A conversion failure now fails first.
* **`NewFile_seek` null-checks its handle** — the one `NewFile_*` entry
  point where the C dereferenced NULL.
* **Handles are read through a checked word.** The C read a pointer-sized
  word from whatever object sat on the stack; this port requires a byte
  object of at least word size (`BadArgument` otherwise). A *forged* handle
  word remains exactly as undefined as in C — same trust model, including
  double-close being a double-free in both.
* The primitives that end in an explicit `pop:` in the shim
  (`FileClose`, `FileSeek`, `DirectoryClose`, `DirectoryRewind`,
  `MemoryUnmap`) check the argument count the pop hard-codes; the C would
  unbalance the stack if the image installed them at another arity. The
  other primitives stay arity-agnostic, as the C's `methodReturn*` calls
  were.

## Accessor depths

The generated `NewFilePlugin.c` is not in the tree, so the exported
`...AccessorDepth` bytes could not be copied; they were computed by applying
Slang's algorithm (`smalltalksrc/Melchor/MLPluginAccessorDepthCalculator`
with `MLAccessorDepthCalculator`) to `NewFilePlugin.class.st` by hand, and
the method calibrated against generated plugins that are in tree
(`UUIDPlugin.c`'s depth-1 `firstIndexableField` chain,
`JPEGReadWriter2Plugin.c`'s −1/2 mix). Result: **1** for the primitives that
bind `firstIndexableField:` of a stack value to a variable (the five
path-taking primitives and the four buffer read/writes), **−1** for
`primitiveIsAvailable` (no stack-rooted assignment at all), **0** for the
rest, where the `cCoerce: (pointerAtPointer: (firstIndexableField: ...))`
nesting keeps the chain at a bare same-level accessor.

## Verification

`cargo test -p new-file-plugin`: 28 tests, all against real files in a
tempdir under `target/tmp`.

* open-mode/creation-disposition matrix: CreateNew's `O_EXCL`,
  CreateAlways' and TruncateExisting's `O_TRUNC`, OpenAlways preserving
  content, OpenExisting and TruncateExisting failing on a missing file, the
  invalid-mode and unknown-disposition paths;
* error paths: missing file, permission-denied (skipped as root),
  write on a read-only descriptor answering the syscall's −1, and every
  NULL-handle convention (−1 / 0 / false / no-op) individually;
* positioned I/O: seek set/current/end + tell, the unknown-mode no-op,
  `pread`/`pwrite` leaving the file position alone, buffer offsets, append
  mode overriding a rewind;
* 64-bit offsets: seek/tell/size/`pread` at 5 GB in a sparse file;
* truncation shrinking and zero-fill extending;
* directories: create/list/rewind/remove-empty, mkdir-exists and
  rmdir-non-empty answering false, path size honoured over the buffer
  length, embedded-NUL truncation;
* memory mapping: contents visible read-only, `MAP_SHARED` write-through,
  the nested-map reference count, the empty-file NULL, and the faithful
  unmap oddity exercised behaviourally (mapping still readable after the
  count reaches zero).

## Not verified

* **Anything through the interpreter proxy.** There is no runnable VM in
  this environment, so the primitive layer — stack offsets, the
  `positive32/64BitValueOf`/`positiveMachineIntegerValueOf`/
  `signed64BitValueOf` conversions and their failure flags,
  `classExternalAddress` instantiation, handle-word traffic, pop/return
  balance — compiles but is untested. An image-side differential pass
  should focus here first.
* The `SIZE_MAX` branch of `NewFile_memoryMap` (a handle whose `fstat`
  fails) — not constructible from a healthy descriptor in a test.
* Behaviour under a 32-bit VM: the crate assumes the 64-bit Unix targets
  the VM ships on (`c_long`/`off_t` = 64 bits) and will not compile
  otherwise.

## Not done yet

* **CMake wiring.** The crate builds and exports the right symbols, but the
  build still compiles the C plugin; switching Unix over to this crate is
  deliberately a separate change. Windows must keep building
  `Win32File.c` + the generated shims regardless.
