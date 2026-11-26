#!/bin/sh
# run.sh IMAGE SCRIPT [timeoutSecs] - runs a Smalltalk script on an image,
# headless, inside [ ... ] on: Error, UnhandledError do: [ print the error and
# 25 frames of its stack, exit 1 ] (trailer.st), so that an error prints and
# quits instead of hanging on a debugger; the image is saved only with SAVE=1,
# and never after an error.
#
#   PHARO_VM   the VM: its directory or its pharo (default: pharo on PATH)
#   SAVE=1     save the image after the script (snapshot: true andQuit: true)
# The script's temporaries, if any, must come first (they become those of the
# block).  The status is the VM's: 0, 1 after an error, 124 after the
# timeout (default 600 s).
HERE=$(cd "$(dirname "$0")" && pwd)
test $# -ge 2 || { sed -n '2,12p' "$0"; exit 2; }
vm=${PHARO_VM:-pharo}
test -d "$vm" && vm=$vm/pharo
image=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
script=$(cd "$(dirname "$2")" && pwd)/$(basename "$2")
T=$(mktemp "${TMPDIR:-/tmp}/run-st.XXXXXX") || exit 2
mv "$T" "$T.st"; T=$T.st
{ echo '[ '; cat "$script"; echo
  if test -n "${SAVE:-}"; then sed 's/snapshot: false/snapshot: true/' "$HERE/trailer.st"; else cat "$HERE/trailer.st"; fi; } > "$T"
cd "$(dirname "$image")" || exit 2
timeout "${3:-600}" "$vm" --headless "$image" --no-default-preferences st "$T" 2>&1
rc=$?
rm -f "$T"
exit $rc
