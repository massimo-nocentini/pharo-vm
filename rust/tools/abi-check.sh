#!/usr/bin/env bash
#
# abi-check.sh -- prove a migration wave did not move the ABI.
#
# The Rust port of the platform layer is strictly symbol-for-symbol: every
# `#[no_mangle] extern "C"` item in pharo-platform replaces a symbol that used
# to come from a .c file in src/, and the linker output must be
# indistinguishable. This script is the mechanical check for that, and it is
# the primary safety net of the migration -- run it on every wave, in CI.
#
# Usage:
#   abi-check.sh capture <library> <baseline-file>
#       Record the dynamic symbols exported by <library>.
#
#   abi-check.sh compare <library> <baseline-file>
#       Fail if <library>'s exported symbols differ from the baseline.
#
# Typical use around a wave:
#
#   cmake -S . -B build-c  -DUSE_RUST_PLATFORM=OFF && cmake --build build-c
#   rust/tools/abi-check.sh capture build-c/build/vm/libPharoVMCore.so abi-baseline.txt
#
#   cmake -S . -B build-rs -DUSE_RUST_PLATFORM=ON  && cmake --build build-rs
#   rust/tools/abi-check.sh compare build-rs/build/vm/libPharoVMCore.so abi-baseline.txt
#
# Note this compares *names*, not signatures. C linkage carries no type
# information, so a changed parameter type passes here and only shows up at
# runtime. That is exactly why the plan pairs this with differential testing
# against a real image rather than treating a green ABI check as sufficient.

set -euo pipefail

usage() {
    sed -n '3,28p' "$0" | sed 's/^# \?//'
    exit 2
}

# Emits the sorted, deduplicated set of globally-visible defined symbols.
#
# Filtered out:
#   - Rust's own runtime plumbing, which is an implementation detail of the
#     staticlib and never part of the VM's contract.
#   - Compiler/linker-synthesised symbols that vary by toolchain.
extract_symbols() {
    local library="$1"

    if [[ ! -f "$library" ]]; then
        echo "abi-check: no such library: $library" >&2
        exit 1
    fi

    # -D reads the dynamic symbol table (what the VM actually exports);
    # --defined-only drops undefined references to libc and friends.
    nm -D --defined-only "$library" \
        | awk '{print $NF}' \
        | grep -Ev "$RUST_INTERNAL_RE" \
        | grep -Ev '^(_init|_fini|__bss_start|_edata|_end|__.*_impl_.*)$' \
        | LC_ALL=C sort -u
}

# Rust's own mangled symbols: `_R...` is the v0 scheme, `_ZN<len>core...` the
# legacy one. These are implementation details of the Rust half, never part of
# the VM's contract, and an all-C build has none of them -- so filtering them
# cannot hide a regression. `rust_internal_count` reports how many there are,
# because the number is worth watching (see below).
RUST_INTERNAL_RE='^(_R|_ZN[0-9]+(core|alloc|std|rustc)|rust_|__rust_)'

# How many Rust-internal symbols the library exports.
#
# Linking a Rust staticlib into the VM's shared library exports Rust's
# internals along with it: pulling in `std::fs` alone drags in the panic and
# backtrace machinery, which is over a thousand symbols. They are harmlessly
# namespaced, but they bloat the dynamic symbol table. The fix is a linker
# version script naming the intended exports; until then, this number makes the
# cost visible instead of hiding it behind the filter.
rust_internal_count() {
    nm -D --defined-only "$1" | awk '{print $NF}' | grep -Ec "$RUST_INTERNAL_RE" || true
}

command="${1:-}"
library="${2:-}"
baseline="${3:-}"

if [[ -z "$command" || -z "$library" || -z "$baseline" ]]; then
    usage
fi

case "$command" in
capture)
    extract_symbols "$library" > "$baseline"
    echo "abi-check: captured $(wc -l < "$baseline") symbols from $library into $baseline"
    echo "abi-check: (plus $(rust_internal_count "$library") Rust-internal symbols, not compared)"
    ;;

compare)
    if [[ ! -f "$baseline" ]]; then
        echo "abi-check: baseline not found: $baseline" >&2
        echo "abi-check: run 'abi-check.sh capture' against an all-C build first." >&2
        exit 1
    fi

    current="$(mktemp)"
    trap 'rm -f "$current"' EXIT
    extract_symbols "$library" > "$current"

    if diff_output="$(diff -u "$baseline" "$current")"; then
        echo "abi-check: OK -- $(wc -l < "$current") exported symbols match $baseline"
        echo "abi-check: (plus $(rust_internal_count "$library") Rust-internal symbols, not compared)"
        exit 0
    fi

    echo "abi-check: FAILED -- exported symbols changed" >&2
    echo >&2
    echo "  '-' lines were exported before and are now missing:" >&2
    echo "      a C file was dropped from SUPPORT_SOURCES without the Rust" >&2
    echo "      module exporting the same symbol (check #[no_mangle])." >&2
    echo >&2
    echo "  '+' lines are newly exported:" >&2
    echo "      usually a Rust helper that should be private, or a symbol" >&2
    echo "      that needs adding to the filters in extract_symbols." >&2
    echo >&2
    echo "$diff_output" >&2
    exit 1
    ;;

*)
    usage
    ;;
esac
