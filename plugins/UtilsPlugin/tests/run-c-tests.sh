#!/bin/sh
# run-c-tests.sh -- the C tests of the UtilsPlugin's engine (upEngine.h),
# without a VM: tests/enginetest.c with src/engine/upEngine.c, against
# Knuth's rng-double.c compiled as he wrote it (knuthref, its main renamed
# on the command line, so that the file stays unchanged).
#
#   sh run-c-tests.sh [--build <dir>] [--cc <compiler>] [--only engine,engine-asan]
#
# Lanes:
#   engine       -O2 -ffp-contract=off, the flags of the plugin
#   engine-asan  the same with AddressSanitizer and UBSan (skipped when the
#                compiler cannot link them)
# Also checks that src/knuth/rng-double.c is Knuth's file (its SHA256).
# Prints one line per lane and exits non-zero when one failed.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
plugin=$(dirname "$here")
build=${TMPDIR:-/tmp}/utilsplugin-ctests.$$
cc=${CC:-cc}
only=engine,engine-asan
while [ $# -gt 0 ]; do
	case $1 in
	--build) build=$2; shift 2 ;;
	--cc) cc=$2; shift 2 ;;
	--only) only=$2; shift 2 ;;
	*) echo "run-c-tests.sh: unknown option $1" >&2; exit 2 ;;
	esac
done
mkdir -p "$build"

knuth_sha256=e8bc0a86c3da0ad50479ced63125c004583459703e88ff276452dc1a48c46fed
if command -v sha256sum >/dev/null 2>&1; then
	sum=$(sha256sum "$plugin/src/knuth/rng-double.c" | cut -d' ' -f1)
else
	sum=$(shasum -a 256 "$plugin/src/knuth/rng-double.c" | cut -d' ' -f1)
fi
if [ "$sum" != "$knuth_sha256" ]; then
	echo "rng-double.c: FAIL, not Knuth's file (SHA256 $sum)"
	exit 1
fi
echo "rng-double.c: Knuth's file"

status=0
lane() {
	name=$1; shift
	case ",$only," in *",$name,"*) ;; *) return 0 ;; esac
	out=$build/$name
	mkdir -p "$out"
	# Knuth's file, as it is: only its main renamed
	if ! "$cc" "$@" -w -Dmain=knuth_rng_double_main -c "$plugin/src/knuth/rng-double.c" -o "$out/knuthref.o" 2>"$out/build.log" \
	|| ! "$cc" "$@" -Wall -Wextra -I"$plugin/include/common" -c "$plugin/src/engine/upEngine.c" -o "$out/upEngine.o" 2>>"$out/build.log" \
	|| ! "$cc" "$@" -Wall -Wextra -I"$plugin/include/common" -c "$here/enginetest.c" -o "$out/enginetest.o" 2>>"$out/build.log" \
	|| ! "$cc" "$@" "$out/enginetest.o" "$out/upEngine.o" "$out/knuthref.o" -lm -o "$out/enginetest" 2>>"$out/build.log"; then
		if [ "$name" = engine-asan ] && grep -q -i 'sanitizer\|asan' "$out/build.log"; then
			echo "$name: skipped (no sanitizers)"
			return 0
		fi
		echo "$name: FAIL (build)"; cat "$out/build.log"; status=1; return 0
	fi
	if [ -s "$out/build.log" ]; then
		echo "$name: warnings"; cat "$out/build.log"
	fi
	if result=$("$out/enginetest" 2>&1); then
		echo "$name: $result"
	else
		echo "$name: FAIL"; echo "$result"; status=1
	fi
}

lane engine -O2 -ffp-contract=off
lane engine-asan -O1 -g -ffp-contract=off -fsanitize=address,undefined -fno-sanitize-recover=all
exit $status
