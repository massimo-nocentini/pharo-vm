# Slang tools: regenerate plugin C, build, and compare against a baseline VM

Written for the backport of the Rust plugins to C (branch pharo-12-backport, 2026-10), where many
agents edited plugins at once; they work on any checkout with a CMake build of the VM.

Paths (`env.sh`, sourced by every tool, sets them; each can be overridden from the environment):

- `TOOLS`: this directory.
- `REPO` (`PHARO_VM_REPO`): the checkout, by default three levels up from here.
- `WORK` (`SLANG_TOOLS_WORK`, default `$TMPDIR/pharo-vm-slang-tools`): run dirs (`runs/`), build
  logs, the regen cache, the build lock and, by default, `baseline-vm`.
- `BUILD` (`SLANG_TOOLS_BUILD`, default `REPO/build-backport`): the CMake build that `build.sh`
  drives; `GEN` (`SLANG_TOOLS_GEN`, default `BUILD-gen/generated`) its generated sources.
- `SLANG_TOOLS_VM` (default `BUILD/build/vm`), `SLANG_TOOLS_BASELINE_VM` (default `WORK/baseline-vm`),
  `SLANG_TOOLS_IMAGES` (default `REPO/build-wasm/images`, the Pharo 12 image of `run-st.sh`),
  `SLANG_TOOLS_VMMAKER` (default `REPO/build-wasm/vmmaker/image`), `SLANG_TOOLS_JOBS` (default 24).

Every tool prints its usage with `-h`. Temp dirs go to `WORK/runs/` and are deleted when the tool exits.

| tool | what it does | typical time |
|---|---|---|
| `build.sh` | builds CMake targets of `REPO/build-backport` under a lock | no-op <1 s; one plugin ~1 s |
| `run-st.sh` | runs a Smalltalk script on a fresh Pharo 12 image, with the new VM or the baseline VM | ~0.2 s |
| `run-diff.sh` | runs the same script on both VMs and diffs the output | ~0.5 s |
| `regen.sh` | regenerates the C of one or more plugins with Slang, from the working tree, another dir, or a git revision | ~8-9 s (7.4 s of that is loading the packages) |
| `regen-diff.sh` | shows only the meaningful differences between the Slang and the C of one plugin | ~16 s the first time, ~8 s after (the base regen is cached) |
| `install-generated.sh` | puts a regenerated FilePlugin, NewFilePlugin, SurfacePlugin or FloatArrayPlugin into the build | ~8 s |
| `normalize-c.sh` | the normalizer that `regen-diff.sh` uses | - |

## The baseline VM

Make it once, before changing any source: `cp -a "$SLANG_TOOLS_VM" "$WORK/baseline-vm"` (in the
backport it was taken at 82c717954, with `git status --short` showing only the two build dirs). Use it
for before/after comparisons and never rebuild it.

At the time of that snapshot, every `.so` and `pharo` in `build-backport/build/vm` was byte-identical to
the baseline. The build is reproducible: a no-op recompile of LargeIntegers or SurfacePlugin produces
an identical `.so`. So `cmp $WORK/baseline-vm/libX.so $REPO/build-backport/build/vm/libX.so` tells
you whether your change reached the binary.

**Pitfall: you must set `LD_LIBRARY_PATH`.** `run-st.sh` does this for you. The `pharo` binary has
`RUNPATH=.:$REPO/build-backport/build/vm`. Started without `LD_LIBRARY_PATH=<vmdir>`, the baseline
`pharo` silently loads the new `libPharoVMCore.so` and the new plugins. Also, libgit2 cannot find
libssl/libssh2, so image startup fails with `SymbolNotFoundError: git_libgit2_init` and exits 1. I
checked with `LD_DEBUG=files` that every library is loaded from the right dir when the variable is set.

## build.sh

```
$TOOLS/build.sh [--fast] [--full-log] <target>...
```

- Runs `flock $TOOLS/build.lock cmake --build $REPO/build-backport --target <targets> -j 24`. The exit
  status is the build's exit status.
- Prints the error/warning lines (with 3 lines of context), the failed make steps, a 12-line tail, and
  the path of the full log in `WORK/runs/build-logs/`.
- Only translation units that were recompiled emit diagnostics. Those are the files changed since the
  last build, which may include files that someone else is editing.
- Target names:
  - plugins: `LargeIntegers`, `BitBltPlugin`, `MiscPrimitivePlugin`, `JPEGReaderPlugin`,
    `JPEGReadWriter2Plugin`, `LocalePlugin`, `SocketPlugin`, `DSAPrims`, `B2DPlugin`, `FilePlugin`,
    `NewFilePlugin`, `SurfacePlugin`, `FloatArrayPlugin`, `FileAttributesPlugin`,
    `UnixOSProcessPlugin`, `SimdJSONPlugin`
  - `PharoVMCore` for `src/`
  - `pharo`, which builds everything, all plugins included
- SqueakSSL and UUIDPlugin are not built (`FEATURE_PLUGIN_SSL` and `FEATURE_PLUGIN_UUID` are OFF).
- `--fast` builds `<target>/fast` with make, which skips the target's dependencies such as
  PharoVMCore. Use it when someone else is in the middle of editing `src/` and you only need your
  plugin rebuilt.
- The output `.so` files go straight into `REPO/build-backport/build/vm/`.

## run-st.sh

```
$TOOLS/run-st.sh [--baseline] [--vm DIR] [--keep] [--eval] <script.st | 'expr'> [timeoutSecs=120]
```

- Copies a fresh `Pharo12.0-SNAPSHOT-64bit-7d5f14cb47.image`/`.changes` and
  `Pharo12.0-64bit-7d5f14c.sources` from `REPO/build-wasm/images/` into a new
  `WORK/runs/run.XXXXXX`.
- Runs `LD_LIBRARY_PATH=<vm> <vm>/pharo --headless <image> st --quit <script>` there. With `--eval`
  it runs `... eval '<expr>'` instead, which prints the printString of the result.
- The default VM is `REPO/build-backport/build/vm`. `--baseline` selects `WORK/baseline-vm`.
- Prints stdout, then stderr (after `----- stderr -----`), then a final line:
  `[run-st] vm=<new|baseline> exit=<n> status=<OK|ERROR|SEGFAULT|CRASH(signal n)|TIMEOUT> time=<s>`
- Exits with the VM's exit status. The temp dir is always deleted; `--keep` keeps it and prints its
  path.

Exit codes, as measured:

| case | exit | status | notes |
|---|---|---|---|
| success | 0 | OK | |
| uncaught Error in the script | 1 | ERROR | the error and its stack go to stderr; PharoDebug.log is printed; the rest of the script does not run |
| segfault | 255 | SEGFAULT | the VM traps SIGSEGV, prints `Segmentation fault <date>` plus C and Smalltalk stacks to stderr, writes crash.dmp, exits 255; `[run-st] crash: ...` is printed |
| timeout | 124 | TIMEOUT | `timeout --kill-after=5`, so it never hangs |

Writing scripts:

- **Output.** `Stdio stdout nextPutAll: 'x=', v printString; lf; flush.` is the reliable way.
  `Transcript show: '...'; cr; flush` also reaches stdout when headless.
- **`!` must be doubled.** `st` reads the file in chunk format, so a single `!` ends the chunk:
  `#!` becomes `#!!`, and `'a!b'` becomes `'a!!b'`. Temporaries declared in one chunk are not visible
  in the next.
- **Finishing.** `st --quit` quits after the script, so do not save the image.
  `Smalltalk exitFailure` or `Smalltalk exit: n` sets the exit code.
- **Example.** `$TOOLS/run-st.sh --eval '1000 factorial printString size'` prints `2568` on both VMs.
  To check which modules are loaded: `Smalltalk vm listLoadedModules`.

## run-diff.sh

```
$TOOLS/run-diff.sh [--eval] <script.st | 'expr'> [timeoutSecs]
```

Runs `run-st.sh --baseline` and then `run-st.sh`, and prints both status lines and a unified diff of
the full outputs. It normalizes timings, the `vm=` label and the VM/run dir paths. Exit 0 means the
outputs are identical, 1 means they differ. Crash dumps still differ in addresses and dates.

## regen.sh: regenerate plugin C with Slang

```
$TOOLS/regen.sh [--src DIR | --rev GITREV] [--keep] [--strict] <PluginClassName>[,<Other>...] <outdir>
```

How it works:

1. Copies `REPO/build-wasm/vmmaker/image/VMMaker.image`, its `.changes` and `Pharo13.1-64bit-e84a2d1.sources`
   into a temp dir. The original is never written.
2. Runs the copy on the baseline VM.
3. Loads these packages with Monticello (`TonelRepository` plus one `MCVersionLoader`), from
   `REPO/smalltalksrc` (the working tree, by default): Printf, VMMakerLoadingDependencies, CAST, Slang,
   Melchor, VMMaker and FileAttributesPlugin. Monticello also removes methods that no longer exist in
   the source. Metacello is not used, because the baseline pulls GitHub dependencies.
   - `--src DIR` loads another smalltalksrc dir instead.
   - `--rev REV` loads `git archive REV smalltalksrc` and does not touch the working tree.
4. Translates each plugin the way `PharoVMMaker` does: CoInterpreter, `Spur64BitCoMemoryManager`,
   `VMMaker generateExternalPlugin:`.

Output:

- One plugin: `<outdir>/<moduleName>.c`.
- A comma list: `<outdir>/<moduleName>/<moduleName>.c` for each, loading only once.
- The last line(s) are `REGEN-OUTPUT: <path>`.

Class name to module name:

| class | module |
|---|---|
| LargeIntegersPlugin | LargeIntegers |
| BitBltSimulation | BitBltPlugin |
| SqueakSSLPlugin | SqueakSSL |
| all others | same name |

These are checked: BitBltSimulation, MiscPrimitivePlugin, JPEGReaderPlugin, JPEGReadWriter2Plugin,
LocalePlugin, SocketPlugin, SqueakSSLPlugin, UUIDPlugin, FileAttributesPlugin, LargeIntegersPlugin,
FilePlugin, NewFilePlugin, SurfacePlugin and FloatArrayPlugin. All 14 regenerate in one 12 s run.

**DSAPrims and B2DPlugin have no Slang source in this repo**, so for them you edit the checked-in C
only.

Errors make it exit non-zero and print:

- `REGEN-LOAD-ERROR`, for example on a syntax error in your `.st`. It gives the class, the method's
  first line and the text near the error.
- `REGEN-TRANSLATE-ERROR`, with a short stack and `while translating method: Class>>selector`.

**Lenient mode (the default; `--strict` turns it off).** This Pharo Slang cannot translate
BitBltSimulation at HEAD, so the throw-away image gets two patches. Each one is logged as a
`REGEN-LENIENT:` line.

1. A `<var: #x type: ...>` that names a non-local variable is normally a TranslationError. Lenient
   mode drops the declaration instead. This happens in `BitBltSimulation>>alphaBlendConst:with:paintMode:`
   for the inst var `sourceAlpha`, and matches the checked-in C, where `sourceAlpha` stays a global
   `sqInt`.
2. Dead-code elimination can strip both arms of a conditional. Lenient mode then emits an empty
   statement instead of crashing with `SubscriptOutOfBounds` in `generateCASTIfFalse:`.

**Caveat:** in the BitBlt output, `copyLoopPixMap` has lost its inlined `pickSourcePixels` loop. This
is a Slang dead-code bug, so the regenerated BitBltPlugin.c is **not** a usable C file. It is still
fine for the Slang delta in `regen-diff.sh`: the same generator runs on both sides, and the
non-inlined `pickSourcePixels...` function is still emitted. Only BitBlt hits either patch; every
other plugin translates the same way in strict mode.

## regen-diff.sh: the meaningful differences only

```
$TOOLS/regen-diff.sh [--base REV] [--raw] <PluginClassName> <c-file>
# e.g. regen-diff.sh LargeIntegersPlugin plugins/LargeIntegers/src/common/LargeIntegers.c
#      regen-diff.sh FilePlugin build-backport-gen/generated/64/plugins/src/FilePlugin/FilePlugin.c
```

`<c-file>` may be relative to REPO. `--base` defaults to HEAD. The output has three sections:

1. **SLANG DELTA**: `regen(base smalltalksrc)` compared with `regen(working tree)`. This is exactly
   what your `.st` edit changes in the generated C, with no noise. The base regen is cached in
   `WORK/runs/regen-cache/<sha>/<Plugin>/`.
2. **C DELTA**: what changed in `<c-file>`.
   - For a tracked file, this is `git diff base -- file`.
   - For a build-generated file, which is untracked, it is `regen(base)` compared with the file.
3. **CHECK**:
   - **Build-generated plugins (FilePlugin, NewFilePlugin, SurfacePlugin, FloatArrayPlugin):** the
     installed file must be byte-identical to `regen(working tree)`. Run `install-generated.sh` to get
     there.
   - **Checked-in plugins:** the `+` and `-` lines of [1] and [2] are normalized and compared as sets.
     `OK: the normalized line sets of [1] and [2] match` means your hand edit to the C mirrors the
     `.st` edit line for line.
     Otherwise the lines that appear in only one delta are listed. Expect cosmetic leftovers: cast or
     parenthesis style, and an edit inlined into several callers shows up several times in [1].

`--raw` also prints the full normalized diff between `<c-file>` and `regen(working tree)`.
`--src DIR` and `--c-base FILE` exist for testing.

### Known noise when you diff regenerated C against the checked-in C

**Build-generated plugins:** `regen(working tree)` of the unmodified tree is **byte-identical** to
`build-backport-gen/generated/64/plugins/src/<P>/<P>.c`. This holds for FilePlugin, NewFilePlugin,
SurfacePlugin and FloatArrayPlugin. There is zero noise.

**Checked-in plugins** were produced by the old Squeak `VMMaker.oscog-eem.2495` and differ heavily.
Diff sizes are in `<`/`>` lines, measured on the unmodified tree:

| plugin | raw | after normalize-c.sh + diff -w | file lines |
|---|---|---|---|
| LargeIntegers | 4406 | 2237 | 2431 |
| BitBltPlugin | 9691 | 3514 | 8142 |
| SocketPlugin | 2815 | 977 | 2439 |
| JPEGReaderPlugin | 1993 | 1100 | 1081 |
| MiscPrimitivePlugin | 939 | 240 | 807 |
| FileAttributesPlugin | 1398 | 405 | 1200 |
| JPEGReadWriter2Plugin | 610 | 204 | 569 |
| LocalePlugin | 377 | 83 | 421 |
| SqueakSSL | 538 | 71 | 587 |
| UUIDPlugin (plugins/UUIDPlugin/common) | 256 | 204 | 171 |

Sources of this noise:

- **Removed by `normalize-c.sh`:**
  - the header comment (`SmartSyntaxPluginCodeGenerator VMMaker.oscog-eem.2495 uuid: ...` versus
    `Automatically generated by Slang`)
  - the `__buildInfo` line
  - `VMMaker.oscog-eem.2495` inside the moduleName string
  - the `_` prefix that Pharo Slang adds to locals and arguments (`_anOop`)
  - `#endif /* cond */` comments
  - blank lines and trailing whitespace
- **Real code-shape differences, not normalized:**
  - different inlining decisions (the new Slang keeps `cDigit*`, `digit*Large*`, `msg`, `sqAssert` and
    others as separate static functions)
  - extra or removed interpreterProxy function pointers
  - cast and parenthesis style
  - local variable order

So never diff a checked-in plugin directly against regen output. Use the SLANG DELTA ([1]) and
mirror it by hand.

## install-generated.sh

```
$TOOLS/install-generated.sh FilePlugin|NewFilePlugin|SurfacePlugin|FloatArrayPlugin [regenerated.c]
$TOOLS/build.sh <Plugin>
```

Without a file argument, it regenerates the plugin from the working tree first. It then shows the
diff and copies the file, under the build lock, to
`REPO/build-backport-gen/generated/64/plugins/src/<P>/<P>.c`. That file is what the build-backport
target compiles.

**Note:** `build-backport-gen/generated` *was a symlink to `REPO/build/generated`* in the backport, which was
replaced with a real copy (`cp -a`, so timestamps were kept and nothing rebuilt). Writes into it no
longer touch the user's tree, and the script refuses to write if it is a symlink again. The VM core
sources in there (`generated/64/vm/src`) are from the same Sep 10 generation; nothing regenerates them.

To restore a generated file, re-run `install-generated.sh <P>` against the unmodified `.st`, or pass
the old file. Do not use `touch -r` to restore an old mtime, because make will then skip the
recompile.

## Verified end to end (2026-10-26)

- **Baseline snapshot.** Runs. `LD_DEBUG` confirmed all libraries load from `baseline-vm`.
- **run-st.sh.** `1000 factorial printString size` gives 2568 on both VMs.
  - An `Error` gives exit 1 / ERROR.
  - A read from address 16 gives exit 255 / SEGFAULT on both VMs.
  - `[true] whileTrue` with a 5 s timeout gives 124 / TIMEOUT after 5.0 s.
  - The chunk `!` pitfall was reproduced.
- **run-diff.sh.** Identical output for factorial on both VMs.
- **build.sh.**
  - No-op `LargeIntegers` (exit 0).
  - A forced recompile of `LargeIntegers.c` after deleting its `.o` gives an identical `.so`.
  - `--fast` works.
  - A bad target gives exit 2.
  - `pharo` builds everything.
- **regen.sh.**
  - FilePlugin from the working tree is byte-identical to `build-backport-gen`.
  - With a modified copy of smalltalksrc (`--src`), the edit appears in the regenerated C, which
    proves the reload works.
  - A syntax error gives `REGEN-LOAD-ERROR` with its location, and exit 1.
  - All 14 plugins regenerate.
- **regen-diff.sh.** Tested both cases on the unmodified tree, then a matching .st/.c edit pair
  (`OK`) and a mismatching pair, where the differing lines are listed.
- **install-generated.sh.** No-op detection, installing a test file, rebuilding `SurfacePlugin`, and
  restoring it (the `.so` is back to byte-identical with the baseline).
