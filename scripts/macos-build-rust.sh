#!/bin/sh
#
# Builds the VM with the Rust platform layer and plugins, then deploys
# Pharo.app into /Applications (override with DEST=... in the environment).
#
# set -e is load-bearing: without it a failed build fell through to the deploy
# step, which had already removed /Applications/Pharo.app and then replaced it
# with whatever half-finished bundle the install had managed to write.
set -e

CMAKE=`brew --prefix cmake`/bin/cmake
DEST=${DEST:-/Applications}
APP=Pharo.app
DIST=build/build/dist/${APP}

rm -rf build
${CMAKE} -S . -B build -DCMAKE_BUILD_TYPE=Release -DUSE_RUST_PLATFORM=ON -DUSE_RUST_PLUGINS=ON -DPHARO_DEPENDENCIES_PREFER_DOWNLOAD_BINARIES=TRUE -DCMAKE_C_COMPILER=${CC} -DCMAKE_CXX_COMPILER=${CXX} -DICEBERG_DEFAULT_REMOTE=httpsUrl
${CMAKE} --build build
${CMAKE} --install build

# Nothing under ${DEST} is touched until the freshly installed bundle has been
# shown to run.
"${DIST}/Contents/MacOS/Pharo" --version

# ditto rather than cp -r, since it is the copy that keeps a bundle's metadata
# -- permissions, ACLs and extended attributes -- intact. The rm is needed
# either way: ditto merges into an existing destination rather than replacing
# it, so stale plugins from an earlier build would survive.
rm -rf "${DEST}/${APP}"
ditto "${DIST}" "${DEST}/${APP}"

# The linker signs each Mach-O ad-hoc, but nothing seals the bundle itself, so
# as installed it fails verification with "code has no resources but signature
# indicates they must be present" and LaunchServices refuses to open it from
# Finder. Re-sign ad-hoc, which is all a locally built VM needs.
codesign --force --sign - "${DEST}/${APP}"
codesign --verify --strict "${DEST}/${APP}"

"${DEST}/${APP}/Contents/MacOS/Pharo" --version
