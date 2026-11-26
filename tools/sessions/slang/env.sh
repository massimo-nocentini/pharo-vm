# env.sh - the paths of the Slang tools, sourced by each of them.  Every one
# can be set in the environment; the defaults suit a checkout where
# build-backport is a CMake build of the VM (with VMMaker generated C in
# build-backport-gen) and make wasm has fetched build-wasm/images and
# build-wasm/vmmaker.
#
#   PHARO_VM_REPO             the checkout (default: three levels up from here)
#   SLANG_TOOLS_WORK          run dirs, build logs, the regen cache, the lock,
#                             and baseline-vm (default: $TMPDIR/pharo-vm-slang-tools)
#   SLANG_TOOLS_BUILD         the CMake build that build.sh drives (default: $REPO/build-backport)
#   SLANG_TOOLS_GEN           the generated sources of that build (default: $BUILD-gen/generated)
#   SLANG_TOOLS_VM            the VM with the changes (default: $BUILD/build/vm)
#   SLANG_TOOLS_BASELINE_VM   a copy of the VM before the changes (default: $WORK/baseline-vm;
#                             make it with cp -a "$SLANG_TOOLS_VM" before changing anything)
#   SLANG_TOOLS_IMAGES        where the Pharo 12 image of run-st.sh is (default: $REPO/build-wasm/images)
#   SLANG_TOOLS_VMMAKER       the VMMaker image dir of regen.sh (default: $REPO/build-wasm/vmmaker/image)
#   SLANG_TOOLS_JOBS          parallel jobs of build.sh (default: 24)
TOOLS="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="${PHARO_VM_REPO:-$(cd "$TOOLS/../../.." && pwd)}"
WORK="${SLANG_TOOLS_WORK:-${TMPDIR:-/tmp}/pharo-vm-slang-tools}"
BUILD="${SLANG_TOOLS_BUILD:-$REPO/build-backport}"
GEN="${SLANG_TOOLS_GEN:-$BUILD-gen/generated}"
NEWVM="${SLANG_TOOLS_VM:-$BUILD/build/vm}"
BASEVM="${SLANG_TOOLS_BASELINE_VM:-$WORK/baseline-vm}"
IMGS="${SLANG_TOOLS_IMAGES:-$REPO/build-wasm/images}"
VMI="${SLANG_TOOLS_VMMAKER:-$REPO/build-wasm/vmmaker/image}"
JOBS="${SLANG_TOOLS_JOBS:-24}"
mkdir -p "$WORK"
