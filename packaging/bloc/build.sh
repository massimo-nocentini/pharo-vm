#!/bin/sh
# build.sh - build the Bloc image: a stock Pharo 15 image with Bloc and
# Toplo (load.st), checked by smoke.st, zipped (docs/Bloc.md)
#
#   sh packaging/bloc/build.sh <vm-dir> <Pharo15 image zip> <out.zip>
#
# It is packaging/batteries/build.sh with this directory's load.st and
# smoke.st, whose lines begin with Bloc, and the name Pharo15-bloc: the
# same arguments, the same steps (load and save, smoke on the image saved,
# zip) and the same environment (BATTERIES_NAME, BATTERIES_WORK,
# BATTERIES_TIMEOUT), with BLOC_TOPLO and BLOC_TOPLO_GROUPS of load.st.
# The status is 0 when load.st printed 'Bloc loaded ... ok', smoke.st
# 'Bloc smoke ... ok' and the zip was written, else 1 (2 for a usage error).

HERE=$(cd "$(dirname "$0")" && pwd) || exit 2
BATTERIES_SCRIPTS=$HERE
BATTERIES_MARK=Bloc
BATTERIES_DEFAULT_NAME=Pharo15-bloc
export BATTERIES_SCRIPTS BATTERIES_MARK BATTERIES_DEFAULT_NAME
exec sh "$HERE/../batteries/build.sh" "$@"
