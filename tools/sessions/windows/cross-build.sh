#!/usr/bin/env bash
# Cross-compile the Pharo VM (CoInterpreter) for Windows from Linux with
# llvm-mingw, reusing already generated C sources (no VMMaker run).
#
# usage: cross-build.sh <triple> <build-dir> [extra cmake args...]
#   <triple>     aarch64-w64-mingw32 | x86_64-w64-mingw32
#   <build-dir>  out-of-tree build directory (created; never inside the repo)
#
# Environment overrides:
#   SRC         repository              (default: this checkout, three levels up)
#   GENERATED   dir holding generated/64 (default: $SRC/build, read only)
#   LLVM_MINGW  llvm-mingw root          (default: $HERE/llvm-mingw/llvm-mingw-20261020-ucrt-ubuntu-22.04-x86_64,
#               the release of https://github.com/mstorsjo/llvm-mingw extracted next to this script)
#   GENERATOR   CMake generator          (default: Ninja if found, else Unix Makefiles)
#   JOBS        parallel build jobs      (default: nproc)
#   FFI         ON to keep FEATURE_FFI/THREADED_FFI/PHARO_VM_IN_WORKER_THREAD (default OFF)
#   LIBS        ON to keep FEATURE_LIB_{SDL2,CAIRO,FREETYPE2,GIT2} (default OFF)
#   PROCESSOR   CMAKE_SYSTEM_PROCESSOR (default: ARM64 / AMD64, as MSYS2's native CMake reports)
#   DEPS        space-separated prefixes of cross-built libraries (e.g. libffi), added
#               to CMAKE_FIND_ROOT_PATH; with FFI=ON pass also -DFFI_DIR=<prefix>
#   CMAKE_BIN   directory of the cmake/cpack to use (default: the ones on PATH);
#               the runtime-dependency bundling of cmake/Windows.cmake needs
#               CMake >= 3.31 to read llvm-objdump output (MSYS2 ships 4.x)
#   PACKAGE     1 to run "cpack -G ZIP" after a successful build (the CI zip)
#   STRIP_MSSE2 1 to compile through wrappers that drop -msse2 (default 1 for aarch64)
#
# Extra args after <build-dir> are passed to the configure step, e.g.
#   -DFEATURE_PLUGIN_SIMDJSON=OFF  -DFFI_DIR=/path/to/cross/libffi
set -u -o pipefail

HERE=$(cd "$(dirname "$0")" && pwd)
TRIPLE=${1:?usage: $0 <triple> <build-dir> [cmake args...]}
BUILD=${2:?usage: $0 <triple> <build-dir> [cmake args...]}
shift 2

SRC=${SRC:-$(cd "$HERE/../../.." && pwd)}
GENERATED=${GENERATED:-$SRC/build}
LLVM_MINGW=${LLVM_MINGW:-$HERE/llvm-mingw/llvm-mingw-20261020-ucrt-ubuntu-22.04-x86_64}
JOBS=${JOBS:-$(nproc)}
[ -n "${CMAKE_BIN:-}" ] && export PATH="$CMAKE_BIN:$PATH"
FFI=${FFI:-OFF}
LIBS=${LIBS:-OFF}

case "$TRIPLE" in
  aarch64-w64-mingw32) PROCESSOR=${PROCESSOR:-ARM64} ;;
  x86_64-w64-mingw32)  PROCESSOR=${PROCESSOR:-AMD64} ;;
  *) echo "unsupported triple: $TRIPLE" >&2; exit 2 ;;
esac

if [ -z "${GENERATOR:-}" ]; then
  if command -v ninja >/dev/null 2>&1; then GENERATOR=Ninja; else GENERATOR="Unix Makefiles"; fi
fi

for f in "$LLVM_MINGW/bin/$TRIPLE-clang" "$GENERATED/generated/64/vm/src/cogit.c"; do
  [ -e "$f" ] || { echo "missing: $f" >&2; exit 2; }
done
case "$(realpath -m "$BUILD")/" in
  "$(realpath "$SRC")"/*) echo "refusing to build inside the repository: $BUILD" >&2; exit 2 ;;
esac

mkdir -p "$BUILD"
BUILD=$(realpath "$BUILD")
TOOLCHAIN="$BUILD/toolchain-$TRIPLE.cmake"

# $BUILD/bin plays the role of MSYS2's <prefix>/bin (/clangarm64/bin): it
# holds the compilers (CMake's C compiler directory), the target's runtime
# DLLs (libc++.dll, libunwind.dll, libwinpthread-1.dll) and an "objdump" for
# PE files, which is what cmake/Windows.cmake looks for next to the compiler
# when it bundles the runtime dependencies at install (CPack) time.  The
# compilers are exec wrappers, not symlinks: llvm-mingw's clang finds its
# sysroot from its own directory.
#
# The Windows COMMON_FLAGS of CMakeLists.txt hardcode -msse2, which clang
# rejects for aarch64 ("unsupported option '-msse2' for target ..."), so every
# try_compile of the configure step fails (SIZEOF_VOID_P comes out empty).
# Until CMakeLists.txt adds -msse2 for x86_64 only, STRIP_MSSE2=1 (the default
# for aarch64) makes the wrappers drop that one flag.
STRIP_MSSE2=${STRIP_MSSE2:-$([ "$TRIPLE" = aarch64-w64-mingw32 ] && echo 1 || echo 0)}
mkdir -p "$BUILD/bin"
for c in clang clang++; do
  {
    echo '#!/usr/bin/env bash'
    if [ "$STRIP_MSSE2" = 1 ]; then
      echo 'args=(); for a in "$@"; do [ "$a" = -msse2 ] || args+=("$a"); done; set -- "${args[@]}"'
    fi
    echo "exec \"$LLVM_MINGW/bin/$TRIPLE-$c\" \"\$@\""
  } > "$BUILD/bin/$TRIPLE-$c"
  chmod +x "$BUILD/bin/$TRIPLE-$c"
done
ln -sf "$LLVM_MINGW/bin/llvm-objdump" "$BUILD/bin/objdump"
cp -f "$LLVM_MINGW/$TRIPLE"/bin/*.dll "$BUILD/bin/"   # copies: install() would bundle symlinks as symlinks
CC_BIN="$BUILD/bin/$TRIPLE-clang"
CXX_BIN="$BUILD/bin/$TRIPLE-clang++"
[ "$STRIP_MSSE2" = 1 ] && echo "note: the compiler wrappers in $BUILD/bin drop -msse2"

# CMAKE_SYSTEM_PROCESSOR is what a native MSYS2 CMake reports on that host
# (ARM64 on Windows arm64, AMD64 on x86_64), so the VM's CMakeLists takes the
# same branches as on the CI runner.
cat > "$TOOLCHAIN" <<EOF
set(CMAKE_SYSTEM_NAME Windows)
set(CMAKE_SYSTEM_PROCESSOR $PROCESSOR)
set(CMAKE_C_COMPILER   "$CC_BIN")
set(CMAKE_CXX_COMPILER "$CXX_BIN")
set(CMAKE_RC_COMPILER  "$LLVM_MINGW/bin/$TRIPLE-windres")
set(CMAKE_AR           "$LLVM_MINGW/bin/$TRIPLE-ar" CACHE FILEPATH "")
set(CMAKE_RANLIB       "$LLVM_MINGW/bin/$TRIPLE-ranlib" CACHE FILEPATH "")
set(CMAKE_OBJDUMP      "$LLVM_MINGW/bin/$TRIPLE-objdump" CACHE FILEPATH "")
set(CMAKE_FIND_ROOT_PATH "$LLVM_MINGW/$TRIPLE" ${DEPS:-})
set(CMAKE_FIND_ROOT_PATH_MODE_PROGRAM NEVER)
set(CMAKE_FIND_ROOT_PATH_MODE_LIBRARY ONLY)
set(CMAKE_FIND_ROOT_PATH_MODE_INCLUDE ONLY)
set(CMAKE_FIND_ROOT_PATH_MODE_PACKAGE ONLY)
EOF

CONFIGURE=(cmake -S "$SRC" -B "$BUILD" -G "$GENERATOR"
  -DCMAKE_TOOLCHAIN_FILE="$TOOLCHAIN"
  -DCMAKE_BUILD_TYPE=RelWithDebInfo
  -DFLAVOUR=CoInterpreter
  -DGENERATE_SOURCES=OFF
  -DGENERATED_SOURCE_DIR="$GENERATED"
  -DFEATURE_FFI="$FFI"
  -DFEATURE_THREADED_FFI="$FFI"
  -DPHARO_VM_IN_WORKER_THREAD="$FFI"
  -DFEATURE_LIB_SDL2="$LIBS"
  -DFEATURE_LIB_CAIRO="$LIBS"
  -DFEATURE_LIB_FREETYPE2="$LIBS"
  -DFEATURE_LIB_GIT2="$LIBS"
  -DICEBERG_DEFAULT_REMOTE=httpsUrl
  -DBUILD_WITH_GRAPHVIZ=OFF
  "$@")

echo "+ ${CONFIGURE[*]}"
"${CONFIGURE[@]}" 2>&1 | tee "$BUILD/configure.log"
rc=${PIPESTATUS[0]}
[ "$rc" -eq 0 ] || { echo "configure failed ($rc), see $BUILD/configure.log" >&2; exit "$rc"; }

# -k 0 / -k: keep going so every failing translation unit is reported
if [ "$GENERATOR" = Ninja ]; then KEEP=(-- -k 0); else KEEP=(-- -k); fi
echo "+ cmake --build $BUILD -j $JOBS ${KEEP[*]}"
cmake --build "$BUILD" -j "$JOBS" "${KEEP[@]}" 2>&1 | tee "$BUILD/build.log"
rc=${PIPESTATUS[0]}
echo "build exit status: $rc (log: $BUILD/build.log)"
if [ -d "$BUILD/build/vm" ]; then
  find "$BUILD/build/vm" -maxdepth 1 \( -name '*.exe' -o -name '*.dll' \) -exec file {} + | sed "s|$BUILD/||"
fi
if [ "$rc" -eq 0 ] && [ "${PACKAGE:-0}" = 1 ]; then
  echo "+ (cd $BUILD && cpack -G ZIP)"
  (cd "$BUILD" && cpack -G ZIP) 2>&1 | tee "$BUILD/cpack.log"
  rc=${PIPESTATUS[0]}
  echo "cpack exit status: $rc"
  ls "$BUILD"/*.zip 2>/dev/null
fi
exit "$rc"
