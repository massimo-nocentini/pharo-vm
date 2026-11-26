#!/usr/bin/env bash
# build.sh [make args...] - make wasm of a checkout or worktree, into its own
# build directory, under a lock of that directory (two builds of one
# directory wait for each other; builds of different directories run at once)
#
# Every WASM_* variable of GNUmakefile is read from the environment (or given
# as an argument).  The configuration of the site of GitHub Actions, as used
# for the Bloc, WebFetch, Server-Sent Events and WebSocket work:
#
#   WASM_SRC=<worktree> WASM_BUILDDIR=<dir> \
#   WASM_IMAGE_ZIP=<Pharo15...zip> WASM_IMAGE_ID=p15 \
#   WASM_EXTRA_IMAGES='batteries=<batteries.zip> bloc=<PharoBloc-Pharo15.zip>' \
#   WASM_WORLD_DEFAULT=batteries WASM_LIBGIT2=ON WASM_PANGO=ON WASM_POPPLER=ON \
#   WASM_HOST_PHARO=<native pharo> WASM_GENERATED=<dir with generated/64> \
#   WASM_DEPS_DIR=<shared downloads dir> WASM_JOBS=40 \
#   tools/sessions/wasm/build.sh
#
# WASM_DEPS_DIR shared between build directories saves the downloads;
# WASM_HOST_PHARO and WASM_GENERATED from an earlier build skip VMMaker.
# 'build.sh -n' is make -n.  Paths and toolchain: env.sh.
set -u
. "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/env.sh"
case "${1:-}" in -h|--help) sed -n '2,20p' "$0"; exit 0 ;; esac
mkdir -p "$(dirname "$WASM_BUILDDIR")"
exec flock "$WASM_BUILDDIR.lock" make -C "$WASM_SRC" wasm "$@"
