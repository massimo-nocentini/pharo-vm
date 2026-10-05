#!/bin/sh
# Run the node VM on the image of web/, unpacked into try/ the first time:
#
#   ./node-try.sh eval '3 + 4'
#   ./node-try.sh st --quit script.st
#
# NODE selects node (24.15 or later).
set -e
d=$(cd "$(dirname "$0")" && pwd)
if ! test -f "$d/try/Pharo.image"; then
    mkdir -p "$d/try"
    for f in "$d"/web/image/*.gz; do
	gunzip -c "$f" > "$d/try/$(basename "$f" .gz)"
    done
fi
cd "$d/try"
exec "$d/node/pharo" --headless Pharo.image --no-default-preferences "$@"
