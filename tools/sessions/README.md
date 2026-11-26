# Session tools

Scripts written while working on this fork (the backport of the Rust plugins, the WebAssembly
port, Bloc, WebFetch, Server-Sent Events and WebSockets, the reviews of Containers-Essentials)
that are worth running again. They are helpers around the build and the tests, not part of them:
nothing in `tests/` or the CI calls them. Each one says in its header what it does, its usage and
its environment variables; paths default to this checkout, and every other path comes from an
argument or a variable.

## slang/ — regenerate plugin C with Slang, build, compare VMs ([README](slang/README.md))

- `env.sh`: the paths of the other tools, each from an environment variable (`SLANG_TOOLS_*`).
- `regen.sh`: regenerates the C of plugins from `smalltalksrc` (working tree, a directory or a git revision) on a copy of the VMMaker image.
- `regen-diff.sh`: what a `.st` edit changes in the generated C, and whether a hand edit of a checked-in plugin mirrors it.
- `normalize-c.sh`: removes the noise between the old VMMaker.oscog output and Pharo Slang's, for `regen-diff.sh`.
- `install-generated.sh`: puts a regenerated FilePlugin, NewFilePlugin, SurfacePlugin or FloatArrayPlugin into the build.
- `regen-template.st`: the Smalltalk side of `regen.sh`.
- `build.sh`: builds CMake targets under a lock, printing only errors, warnings and the tail.
- `run-st.sh`: runs a script or an expression on a fresh Pharo 12 image with the new or the baseline VM.
- `run-diff.sh`: runs it on both VMs and diffs the outputs.

## wasm/ — build and test a worktree in its own build directory

- `env.sh`: emsdk and its node, the checkout (`WASM_SRC`), the build directory (`WASM_BUILDDIR`), Playwright (`PW_ROOT`).
- `build.sh`: `make wasm` under a lock of the build directory; its header has the configuration of the site of GitHub Actions.
- `lanes.sh`: `tests/wasm/run-lanes.sh` on that build (all lanes, or the numbers given).
- `spec.sh`: browser specs of `tests/wasm` on its site, each with its log, and their summaries.

## screenshots/ — the pages, driven by Playwright

- `sse-websocket-shots.mjs`: event sources and WebSockets of WebFetch in the Console, the Notebook and the World, against the test servers of `tests/wasm/lib`.
- `cairo-shots.mjs`: the `cairo:` cell of the example notebook in the light and the dark scheme.

## windows/ — the VM for Windows from Linux

- `cross-build.sh`: cross-compiles the VM for Windows arm64 or x86_64 with llvm-mingw, from generated sources.
- `rename.sh`: gives cpack's zips the names of the release, as the CI does.

## native/ — scripts on native images

- `run.sh`: runs a script on an image headless, printing an error and its stack and exiting 1 instead of opening a debugger; `SAVE=1` saves the image.
- `fresh.sh`: a fresh copy of an image from a Pharo zip or a directory, with loaders run on it.
- `trailer.st`: the end of the wrapper of `run.sh`.
- `load-tonel.st`: loads packages of a local Tonel directory with Monticello, without Metacello.
- `load-roassal.st`, `load-ce.st`: RoassalLayouts, RoassalMN and Containers-Essentials from local checkouts with Metacello.

## probes/ — the measures behind statements of the docs

- `node-no-store.mjs`: what node's fetch sends with `cache: 'no-store'` (docs/WebFetch.md).
- `node-header-trim.mjs`: why header values are trimmed of spaces and tabs only (docs/WebFetch.md).
- `browser-cache-replay.mjs`: whether Chromium's cache replays an event stream to a worker (docs/WebFetch.md).
- `node-websocket-close.mjs`: node's WebSocket after a server's close frame, and the BOM (docs/WebSocket.md).
- `node-websocket-deaf.mjs`: how long node lives with a socket whose close is never answered (docs/WebSocket.md).
- `websocket-linger.mjs`: the sockets that linger in `web-socket.js` (docs/WebSocket.md).
- `browser-websocket-cookies.mjs`: the cookies Chromium sends with a WebSocket handshake (docs/WebSocket.md, Security).
- `bloc-cursor.st`: the hand of the world after a Bloc space sets its cursor (docs/Bloc.md).

The probes of WebSockets and event sources need a checkout with `tests/wasm/lib/ws-server.mjs`
and `/sse` (pharo-12-sse, pharo-12-ws and later); they take it as their argument.

## repo/ — working with the repositories

- `git-hunks.py`: lists the hunks of a file's diff and prints a patch of some of them, to commit a file in steps.
- `tonel-dups.py`: the methods that more than one package defines across Tonel trees.
