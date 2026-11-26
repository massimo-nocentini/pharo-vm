# env.sh - the toolchain and paths of the wasm tools, sourced by each of them
#
#   EMSDK        the emsdk checkout (default: $HOME/emsdk, else $HOME/Developer/emsdk);
#                its emsdk_env.sh is sourced, and its node (the one the CI uses,
#                24.19.0 at the time of writing) comes first in PATH
#   NODE         the node of the lanes and specs (default: emsdk's)
#   CC, CXX      the host compilers of the native parts (default: clang, clang++)
#   WASM_SRC     the checkout or worktree to build and test (default: three levels up)
#   WASM_BUILDDIR  the build directory (default: $WASM_SRC/build-wasm); make reads it,
#                with every other WASM_* variable of GNUmakefile, from the environment
#   PW_ROOT      a directory with a Playwright install for the specs: if set,
#                PLAYWRIGHT_MODULE, PLAYWRIGHT_BROWSERS_PATH and LD_LIBRARY_PATH
#                default to $PW_ROOT/pw/node_modules/playwright, $PW_ROOT/pw/browsers
#                and $PW_ROOT/pwdeps/root/usr/lib/x86_64-linux-gnu (the system
#                libraries of Chromium extracted from .debs, for a machine that
#                lacks them)
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_SRC="${WASM_SRC:-$(cd "$HERE/../../.." && pwd)}"
WASM_BUILDDIR="${WASM_BUILDDIR:-$WASM_SRC/build-wasm}"
export WASM_BUILDDIR
if test -z "${EMSDK:-}"; then
  for d in "$HOME/emsdk" "$HOME/Developer/emsdk"; do test -f "$d/emsdk_env.sh" && EMSDK=$d && break; done
fi
if test -n "${EMSDK:-}" && test -f "$EMSDK/emsdk_env.sh"; then
  . "$EMSDK/emsdk_env.sh" >/dev/null 2>&1
  test -n "${EMSDK_NODE:-}" && PATH="$(dirname "$EMSDK_NODE"):$PATH"
fi
NODE="${NODE:-${EMSDK_NODE:-$(command -v node)}}"
export PATH NODE CC="${CC:-clang}" CXX="${CXX:-clang++}"
if test -n "${PW_ROOT:-}"; then
  export PLAYWRIGHT_MODULE="${PLAYWRIGHT_MODULE:-$PW_ROOT/pw/node_modules/playwright}"
  export PLAYWRIGHT_BROWSERS_PATH="${PLAYWRIGHT_BROWSERS_PATH:-$PW_ROOT/pw/browsers}"
  export LD_LIBRARY_PATH="$PW_ROOT/pwdeps/root/usr/lib/x86_64-linux-gnu${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
fi
