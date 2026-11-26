#!/usr/bin/env bash
# lanes.sh [LANE...] - tests/wasm/run-lanes.sh of WASM_SRC on the build in
# WASM_BUILDDIR (all lanes, or the numbers given: lanes.sh 81 84)
#
#   TEST_DIR       where the lanes write (default: a new directory under $TMPDIR)
#   HOST_PHARO     a native Pharo VM, for the lanes that compare with one
#   GEN            the generated sources (default: $WASM_BUILDDIR/host/generated/64)
#   WEB_FETCH_DIR  a checkout of web-fetch.st, for lanes 81 and 84 to run its tests
#   LIBPANGOCAIRO_DIR, LIBPOPPLER_DIR  checkouts for lane 76
# (tests/wasm/run-lanes.sh documents the others.)  Paths and toolchain: env.sh.
set -u
. "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/env.sh"
case "${1:-}" in -h|--help) sed -n '2,11p' "$0"; exit 0 ;; esac
TEST_DIR="${TEST_DIR:-$(mktemp -d "${TMPDIR:-/tmp}/lanes.XXXXXX")}"
echo "lanes.sh: $WASM_SRC on $WASM_BUILDDIR, output in $TEST_DIR" >&2
cd "$WASM_SRC" && exec env WASM_DIR="$WASM_BUILDDIR" SRCDIR="$WASM_SRC" TEST_DIR="$TEST_DIR" \
  sh tests/wasm/run-lanes.sh "$@"
