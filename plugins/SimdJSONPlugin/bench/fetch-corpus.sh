#!/bin/sh
# fetch-corpus.sh - the JSON files of the SimdJSON benchmark
#
# usage: sh plugins/SimdJSONPlugin/bench/fetch-corpus.sh DIR
#
# Downloads the archive of simdjson-data at commit 3519499 (the corpus of
# simdjson's own benchmarks: twitter.json, citm_catalog.json, canada.json...),
# checks its SHA256, and unpacks its jsonexamples directory into DIR, which
# run-bench.sh then takes as --corpus.  GitHub does not promise that the bytes
# of an archive it makes stay the same: SJ_CORPUS_ZIP=<file> takes the archive
# from that file instead, offline.  A DIR that holds twitter.json already is
# left alone.
#
# Environment: SJ_CORPUS_ZIP, and curl or wget for the download.

set -e
commit=351949906abde446f0314bf79606fb5d884f5be7
sha256=a2623f4cbe5ee988c00ff26549dd0e7738b25938fd4d6e5f1f7d10faf748a884
url=https://github.com/simdjson/simdjson-data/archive/$commit.zip

if test $# -ne 1; then
    echo "usage: sh $0 DIR" >&2
    exit 2
fi
dir=$1
if test -f "$dir/twitter.json"; then
    echo "fetch-corpus: $dir has the corpus already"
    exit 0
fi
mkdir -p "$dir"
dir=$(cd "$dir" && pwd)
tmp=$dir/.fetch-corpus
rm -rf "$tmp"
mkdir "$tmp"
trap 'rm -rf "$tmp"' EXIT

if test -n "$SJ_CORPUS_ZIP"; then
    cp "$SJ_CORPUS_ZIP" "$tmp/data.zip"
elif command -v curl >/dev/null 2>&1; then
    curl -fsSL -o "$tmp/data.zip" "$url"
elif command -v wget >/dev/null 2>&1; then
    wget -q -O "$tmp/data.zip" "$url"
else
    echo "fetch-corpus: neither curl nor wget to download $url (or set SJ_CORPUS_ZIP)" >&2
    exit 1
fi

if command -v sha256sum >/dev/null 2>&1; then
    sum=$(sha256sum "$tmp/data.zip" | cut -d ' ' -f 1)
else
    sum=$(shasum -a 256 "$tmp/data.zip" | cut -d ' ' -f 1)
fi
if test "x$sum" != "x$sha256"; then
    echo "fetch-corpus: SHA256 $sum of ${SJ_CORPUS_ZIP:-$url}, expected $sha256" >&2
    exit 1
fi

(cd "$tmp" && unzip -q data.zip "simdjson-data-$commit/jsonexamples/*")
mv "$tmp/simdjson-data-$commit/jsonexamples"/* "$dir"/
echo "fetch-corpus: $(find "$dir" -name '*.json' | wc -l | tr -d ' ') JSON files in $dir (SHA256 $sha256)"
