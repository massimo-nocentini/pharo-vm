#!/bin/sh
# 76-pango-poppler - Pango and Poppler in the VMs (WASM_PANGO, WASM_POPPLER)
#
# build-wasm/node/pharo, then build-wasm/web/pharo-web.js, on copies of the
# prepared image of the world, image/web, whose OSWindow-Web lets Athens make
# a surface without the SurfacePlugin (a build without it uses the stock
# image with plugins/CairoPlugin/tests/web-surface.st filed in, as lane 74),
# with AthensCairoCanvas>>getHandle (Pharo 12's handle) for ci-text.st:
#
#   registry  the registry of each VM has the libraries glib, gobject,
#             harfbuzz, pango, pangocairo, fontconfig, pangoft2 (and
#             poppler_glib with Poppler),
#             and node/ the placeholders the bindings look for and the fonts
#             with their fonts.conf;
#   probe     tests/cairo-stack/ci-text.st, the probe of the native CI jobs,
#             with the node VM: it resolves each library by the image's own
#             finder in the directory of the VM, checks that Pango, PangoCairo
#             and poppler-glib bind the GLib, GObject, HarfBuzz and cairo of
#             the VM (through the dependencies of the registry), draws 'Ag'
#             with Pango on an Athens canvas and renders two PDFs with
#             Poppler, and must print
#                 PangoCairo ok pango <the pinned version> family DejaVu Sans ink <n>
#                 Poppler ok poppler <the pinned version> pages 1 text yes ink <n> base14 text yes ink <n>
#             (or 'Poppler not bundled' without WASM_POPPLER), with nothing
#             on stderr: no FFI callout failed, adapted or trapped;
#   fonts     the same, with a HOME of its own: fontconfig writes no cache
#             (the fonts.conf of the VM names no cache directory);
#   web       ci-text.st in pharo-web (the MEMORY64=2 module of the pages),
#             through packaging/emscripten/web/vm-driver.js and the REPL of
#             the Console page, as lane 74, with the placeholders the worker
#             writes from manifest.libraries: the fonts are those embedded in
#             its file system (/usr/share/fonts/pharo);
#   bindings  with LIBPANGOCAIRO_DIR (a checkout of libpangocairo.st), its
#             ci/run.sh on the node VM and a copy of the image of the lane:
#             it loads the bindings, lints them and runs their tests, and
#             must print its summary line ending in ' ok' (but for the
#             failure of GLibTest>>testInstanceCheckMismatchOnCopiedCairo,
#             which expects a copy of libcairo.so.2 to be a second cairo:
#             in the VM it names the one cairo); with
#             LIBPOPPLER_DIR too, the same for libpoppler.st (after
#             libpangocairo.st's GLib group).  Skipped without them (the
#             bindings are not submodules of this repository), or for a
#             Pharo older than 12 (Pharo 12 runs them, they are made for 12
#             to 15).
#
# Skipped when the build has no Pango (WASM_PANGO is off).  Each VM run is
# stopped after 600 s (the bindings' after BINDINGS_TIMEOUT, 3600 s); the
# lane prints its time.
#
# Environment (from make wasm-check): NODE, WASM_DIR, SRCDIR, TEST_DIR, and
# WASM_CHECK_TIMEOUT (see lib/common.sh); LIBPANGOCAIRO_DIR,
# LIBPOPPLER_DIR, BINDINGS_TIMEOUT.

SRCDIR=$(cd "${SRCDIR:-$(dirname "$0")/../../..}" && pwd)
TEST_DIR=${TEST_DIR:+$TEST_DIR/76-pango-poppler}
. "$SRCDIR/tests/wasm/lib/common.sh"
lane=pango-poppler
started=$(date +%s)
PROBE=$SRCDIR/tests/cairo-stack/ci-text.st
PINS=$SRCDIR/cmake/cairo-stack/pins.txt
WEB_SURFACE=$SRCDIR/plugins/CairoPlugin/tests/web-surface.st
cache=$WASM_DIR/cmake/CMakeCache.txt

if ! grep -Eiq '^WASM_PANGO:BOOL=(ON|TRUE|YES|Y|1)$' "$cache" 2>/dev/null; then
    skip "the build has no Pango (WASM_PANGO is off)"
fi
poppler=no
if grep -Eiq '^WASM_POPPLER:BOOL=(ON|TRUE|YES|Y|1)$' "$cache" 2>/dev/null; then
    poppler=yes
fi
for f in "$WASM_DIR/node/pharo" "$WASM_DIR/web/pharo-web.js" "$WASM_DIR/web/pharo-web.wasm" "$PROBE" "$PINS"; do
    if ! test -f "$f"; then
	echo "FAIL 76-pango-poppler: no $f"
	exit 1
    fi
done
pango_version=$(awk '$1 == "pango" && NF >= 6 { print $2; exit }' "$PINS")
poppler_version=$(awk '$1 == "poppler" && NF >= 6 { print $2; exit }' "$PINS")
version=$(cat "$WASM_DIR/image/stock/pharo.version" 2>/dev/null)
echo "76-pango-poppler: $WASM_DIR/node/pharo, Pharo $version, Poppler $poppler"

pharo_run() { limit 600 "$WASM_DIR/node/pharo" "$@"; }
P="pharo_run --headless Pharo.image --no-default-preferences"
# the image of the lane: image/web, or the stock one with web-surface.st
base=$TEST_DIR/pango-base
rm -rf "$base"
mkdir -p "$base" || exit 1
if test -d "$WASM_DIR/image/web" && ls "$WASM_DIR/image/web"/*.image >/dev/null 2>&1; then
    copy_image "$WASM_DIR/image/web" "$base" || exit 1
else
    copy_image "$WASM_DIR/image/stock" "$base" || exit 1
    (cd "$base" && $P st --save --quit "$WEB_SURFACE") >"$TEST_DIR/web-surface.out" 2>&1 || {
	echo "FAIL 76-pango-poppler: filing in $WEB_SURFACE"
	cat "$TEST_DIR/web-surface.out"
	exit 1
    }
fi

# ci-text.st reads the cairo_t of an Athens canvas with getHandle, which
# Pharo 12 calls handle
(cd "$base" && $P eval --save "(AthensCairoCanvas canUnderstand: #getHandle) ifFalse: [ AthensCairoCanvas compile: 'getHandle ^ self handle' classified: 'pango-lane' ]. 0") \
    >"$TEST_DIR/get-handle.out" 2>&1 || {
    echo "FAIL 76-pango-poppler: AthensCairoCanvas>>getHandle"
    cat "$TEST_DIR/get-handle.out"
    exit 1
}

# registry
retire
check_dir=
checks=$((checks + 1))
missing=
libraries="glib gobject harfbuzz pango pangocairo fontconfig pangoft2"
files="libglib-2.0.so.0 libgobject-2.0.so.0 libpango-1.0.so.0 libpangocairo-1.0.so.0 libfontconfig.so.1 libpangoft2-1.0.so.0 fonts/fonts.conf fonts/DejaVuSans.ttf fonts/DejaVuSans-Bold.ttf fonts/DejaVuSerif.ttf fonts/DejaVuSerif-Bold.ttf fonts/DejaVuSansMono.ttf"
if test $poppler = yes; then
    libraries="$libraries poppler_glib"
    files="$files libpoppler-glib.so.8"
fi
for vm in node web; do
    for library in $libraries; do
	grep -q "^extern const PharoFFILibrary pharoFFILibrary_$library;\$" "$WASM_DIR/cmake/wasm/ffi/ffiRegistry-$vm.c" \
	    || missing="$missing $library($vm)"
    done
done
for f in $files; do
    test -f "$WASM_DIR/node/$f" || missing="$missing node/$f"
done
for f in libglib-2.0.so.0 libpango-1.0.so.0 libpangocairo-1.0.so.0 libfontconfig.so.1 libpangoft2-1.0.so.0; do
    grep -q "\"$f\"" "$WASM_DIR/web/manifest.json" || missing="$missing manifest.libraries:$f"
done
if test -n "$missing"; then
    fail registry "missing:$missing"
else
    ok registry "$libraries"
fi

pango_line="^PangoCairo ok pango $(echo "$pango_version" | sed 's/\./\\./g') family DejaVu Sans ink [1-9][0-9]*\$"
if test $poppler = yes; then
    poppler_line="^Poppler ok poppler $(echo "$poppler_version" | sed 's/\./\\./g') pages 1 text yes ink [1-9][0-9]* base14 text yes ink [1-9][0-9]*\$"
else
    poppler_line='^Poppler not bundled$'
fi
# the two lines of the probe, in smoke.out; nothing on stderr
probe_lines() {
    if ! grep -Eq "$pango_line" smoke.out; then
	fail "$1" "no line '$pango_line'"
    elif ! grep -Eq "$poppler_line" smoke.out; then
	fail "$1" "no line '$poppler_line'"
    elif test -s smoke.err; then
	fail "$1" "stderr is not empty"
    else
	ok "$1" "$(grep -E '^(PangoCairo|Poppler) ' smoke.out | tr '\n' ';' | sed 's/;$//; s/;/; /')"
    fi
}

fresh probe "$base"
if run probe $P st "$PROBE"; then
    if test "$status" -ne 0; then fail probe "exit status $status"; else probe_lines probe; fi
fi

# fonts: no cache written in an empty HOME (fontconfig's default would be
# $HOME/.cache/fontconfig, XDG_CACHE_HOME unset)
fresh fonts "$base"
mkdir -p home
fonts_run() (
    HOME=$check_dir/home; export HOME
    unset XDG_CACHE_HOME XDG_CONFIG_HOME
    $P st "$PROBE"
)
if run fonts fonts_run; then
    if test "$status" -ne 0; then
	fail fonts "exit status $status"
    elif test -n "$(find home -mindepth 1 | head -n 1)"; then
	fail fonts "the VM wrote in HOME: $(find home -mindepth 1 | head -n 3 | tr '\n' ' ')"
    else
	probe_lines fonts
    fi
fi

# web: the probe evaluated in the REPL of the Console page
web() {
    limit 600 "$NODE" -e '
const fs = require("fs"), path = require("path");
const [srcDir, webDir, imageDir, script] = process.argv.slice(1);
const createPharoVM = require(path.join(webDir, "pharo-web.js"));
const Driver = require(path.join(srcDir, "packaging", "emscripten", "web", "vm-driver.js"));
const manifest = JSON.parse(fs.readFileSync(path.join(webDir, "manifest.json"), "utf8"));
const only = ext => fs.readdirSync(imageDir).filter(f => f.endsWith(ext))[0];
const image = only(".image"), sources = only(".sources");
const dec = { 1: new TextDecoder(), 2: new TextDecoder() };
let out = "", exit = null, crash = null, waiters = [];
const notify = () => { const w = waiters; waiters = []; w.forEach(f => f()); };
const prompt = from => new Promise((resolve, reject) => {
  const check = () => {
    if (crash !== null) return reject(new Error("crash: " + crash));
    if (exit !== null) return reject(new Error("exit " + exit));
    if (out.length > from && out.endsWith("st> ")) return resolve();
    waiters.push(check);
  };
  check();
});
(async () => {
  const drv = await Driver.start(createPharoVM, {
    args: Driver.vmArgs("console"),
    env: {},
    files: [
      { path: "/pharo/Pharo.image", data: fs.readFileSync(path.join(imageDir, image)) },
      { path: "/pharo/Pharo.changes", data: fs.readFileSync(path.join(imageDir, image.replace(/\.image$/, ".changes"))) },
      { path: "/pharo/" + sources, data: fs.readFileSync(path.join(imageDir, sources)) },
      { path: "/pharo/st/web-repl.st", data: fs.readFileSync(path.join(srcDir, "packaging", "emscripten", "st", "web-repl.st")) },
      { path: "/pharo/ci-text.st", data: fs.readFileSync(script) },
    ].concat((manifest.libraries || []).map(name => ({ path: "/pharo/" + name, data: new Uint8Array(0) }))),
    schedule: f => setImmediate(f),
    later: (f, ms) => setTimeout(f, ms),
    cancel: h => clearTimeout(h),
    canRun: () => true,
    onOutput: (fd, bytes) => {
      const text = dec[fd].decode(bytes, { stream: true });
      out += text;
      if (fd === 2) process.stderr.write(text);
      notify();
    },
    onState: notify,
    onHost: () => {},
    onExit: code => { exit = code; notify(); },
    onCrash: message => { crash = message; notify(); },
    onDiag: () => {},
  });
  drv.begin();
  await prompt(0);
  const at = out.length;
  // (the probe without its last statement, which would quit the VM)
  drv.feed("Smalltalk compiler evaluate: ((\x27/pharo/ci-text.st\x27 asFileReference contents) copyReplaceAll: \x27Smalltalk snapshot: false andQuit: true.\x27 with: \x27\x27)\n");
  await prompt(at);
  process.stdout.write(out.slice(at));
  process.exit(0);
})().catch(e => { process.stdout.write(out.slice(-2000)); console.error("web: " + e.message); process.exit(1); });
' "$SRCDIR" "$WASM_DIR/web" "$base" "$PROBE"
}
fresh web "$base"
if run web web; then
    if test "$status" -ne 0; then fail web "exit status $status"; else probe_lines web; fi
fi

# bindings: their own CI scripts on the node VM
bindings_timeout=${BINDINGS_TIMEOUT:-3600}
binding() {   # binding <name> <dir> <summary regex>
    name=$1 dir=$2
    fresh "$name" "$base"
    checks=$((checks + 1))
    if CI_WORK_DIR="$check_dir/work" CI_TIMEOUT=$bindings_timeout PHARO_CAIRO_STACK_EXPECT_LIBS=yes \
	LIBPANGOCAIRO_DIR=$LIBPANGOCAIRO_DIR sh "$dir/ci/run.sh" "$WASM_DIR/node" "$check_dir/Pharo.image" \
	</dev/null >smoke.out 2>smoke.err; then
	ok "$name" "$(grep -E "$3" smoke.out | tail -n 1)"
	return
    fi
    # the tests that cannot pass here: those whose only failures and errors
    # are some of $inapplicable (as many as the summary counts), and nothing
    # else wrong, pass with a note
    summary=$(grep -E "$3" smoke.out | tail -n 1)
    failed=$(grep -E '^(FAIL|ERROR) [A-Za-z0-9_]+>>' smoke.out | sed -E 's/^(FAIL|ERROR) ([^ ]*).*/\2/' | sort -u)
    others=$(echo "$failed" | grep -v -x -F "$inapplicable")
    counted=$(echo "$summary" | sed -nE 's/.* failed ([0-9]+) errors ([0-9]+) [a-z]+ [0-9.]+ FAILED$/\1 + \2/p')
    if test -n "$summary" && test -n "$failed" && test -z "$others" && test -n "$counted" \
	&& test $(($counted)) -eq "$(echo "$failed" | grep -c .)" \
	&& ! grep -Eq '^(UNDECLARED|UNIMPLEMENTED|LINT|SKIP)' smoke.out; then
	ok "$name" "$summary, but for $(echo $failed), which cannot pass in WebAssembly"
    else
	fail "$name" "$(grep -E "$3|^FAIL" smoke.out | tail -n 2 | tr '\n' ' ')"
	grep -E '^(FAIL|ERROR|SKIP|UNDECLARED|UNIMPLEMENTED|LINT)' smoke.out | head -n 40 | sed 's/^/  | /'
    fi
}
# GLibTest>>testInstanceCheckMismatchOnCopiedCairo copies the file of
# libcairo.so.2 elsewhere and expects the check to find a second cairo in
# it: the VM has one cairo, which any copy of its placeholder names (the
# test is for Linux, where a copy is another instance; it should skip on
# Emscripten, as on macOS and Windows).
# PangoMicrodownCompatTest>>testHttpInputFileIsFetchedAsBefore serves the
# file of an inputFile from a ZnServer on 127.0.0.1: the VMs have no
# sockets (NameLookupFailure).
inapplicable='GLibTest>>testInstanceCheckMismatchOnCopiedCairo
PangoMicrodownCompatTest>>testHttpInputFileIsFetchedAsBefore'
if test -z "$LIBPANGOCAIRO_DIR"; then
    skip_check libpangocairo "no LIBPANGOCAIRO_DIR (a checkout of libpangocairo.st)"
elif ! test -f "$LIBPANGOCAIRO_DIR/ci/run.sh"; then
    fail libpangocairo "LIBPANGOCAIRO_DIR=$LIBPANGOCAIRO_DIR has no ci/run.sh"
else
    binding libpangocairo "$LIBPANGOCAIRO_DIR" '^LibPangoCairo [0-9][0-9.]* loaded '
fi
if test $poppler = no; then
    :
elif test -z "$LIBPOPPLER_DIR" || test -z "$LIBPANGOCAIRO_DIR"; then
    skip_check libpoppler "no LIBPOPPLER_DIR and LIBPANGOCAIRO_DIR (checkouts of libpoppler.st and libpangocairo.st)"
elif ! test -f "$LIBPOPPLER_DIR/ci/run.sh"; then
    fail libpoppler "LIBPOPPLER_DIR=$LIBPOPPLER_DIR has no ci/run.sh"
else
    binding libpoppler "$LIBPOPPLER_DIR" '^LibPoppler [0-9][0-9.]* loaded '
fi

rm -rf "$base"
echo "76-pango-poppler: $(($(date +%s) - started)) s"
finish 76-pango-poppler
