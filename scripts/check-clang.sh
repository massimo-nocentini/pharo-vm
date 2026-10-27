#!/bin/sh
# check-clang.sh BUILD_DIR... - fail unless CMake configured each build tree
# with clang for C and clang++ for C++ (compiler id Clang, or AppleClang on
# macOS; emcc and em++ report Clang too), as the CI jobs require
#
#   sh scripts/check-clang.sh build
#
# CMake records the compiler it detected in
# BUILD_DIR/CMakeFiles/<cmake version>/CMake<LANG>Compiler.cmake.

if test $# -eq 0; then
    echo "usage: $0 BUILD_DIR..." >&2
    exit 2
fi
status=0
for dir in "$@"; do
    for lang in C CXX; do
        file=$(ls "$dir"/CMakeFiles/*/CMake${lang}Compiler.cmake 2>/dev/null | tail -n 1)
        id=$(sed -n "s/^set(CMAKE_${lang}_COMPILER_ID \"\(.*\)\")\$/\1/p" "$file" 2>/dev/null)
        compiler=$(sed -n "s/^set(CMAKE_${lang}_COMPILER \"\(.*\)\")\$/\1/p" "$file" 2>/dev/null)
        case $id in
        Clang|AppleClang) echo "$dir: $lang $id ($compiler)" ;;
        *) echo "$dir: $lang is '${id:-unknown}' (${compiler:-no $dir/CMakeFiles/*/CMake${lang}Compiler.cmake}), not clang" >&2; status=1 ;;
        esac
    done
done
exit $status
