#!/bin/bash
# rename.sh - in build/build/packages of the current directory (the CI's
# layout), renames the zips of cpack, PharoVM-<version>+<n>.<commit>-...-bin.zip,
# to the names of the release, PharoVM-<version>-<commit>-...-bin.zip, as the
# jobs of .github/workflows/build.yml do, and fails unless a Windows arm64 zip
# is among them: for a run by hand after cross-build.sh with PACKAGE=1
cd build/build/packages
for zip in PharoVM-*-bin.zip; do mv "$zip" "$(echo "$zip" | sed -E 's/\+[0-9]+\./-/')"; done
ls PharoVM-*-bin.zip
ls PharoVM-*-bin.zip | grep -q -- '-Windows-arm64-bin\.zip$'
