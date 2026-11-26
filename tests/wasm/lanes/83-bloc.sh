#!/bin/sh
# 83-bloc - the Bloc image (packaging/bloc) in the node VM
#
# build-wasm/node/pharo on a copy of image/stock-bloc, the extra image of id
# bloc (WASM_EXTRA_IMAGES=bloc=<the zip of packaging/bloc/build.sh>): Pharo
# 15 with Toplo, Bloc and Alexandrie, whose canvas binds cairo, FreeType and
# HarfBuzz (docs/Bloc.md):
#
#   registry  the registry of each VM has the libraries cairo, freetype and
#             harfbuzz with the functions that only Alexandrie binds (one of
#             each: cairo_push_group, FT_Get_Sfnt_Table, hb_ft_font_create_referenced),
#             node/ has the placeholders that Alexandrie's finders look for
#             (libcairo.so.2, libfreetype.so.6, libharfbuzz.so.0), and so has
#             manifest.libraries;
#   smoke     packaging/bloc/smoke.st: it draws an element, a Toplo button,
#             the pane of the welcome window and into a PharoBlocMorphicForm,
#             and runs the 25 test packages of the image that pass without a
#             display (Bloc-Spec2-Tests among them: spaces in Morphic hosts,
#             whose buffers are PharoBlocMorphicForms here, the VM having no
#             SurfacePlugin); it must print 'PharoBlocMorphicForm needed:
#             true' and its last line ending in ' ok', with no FFI callout
#             failed, adapted or trapped on stderr.
#
# Skipped when the build has no image of id bloc, or no HarfBuzz (WASM_PANGO
# is off).  The smoke is stopped after 1200 s; the lane prints its time.
#
# Environment (from make wasm-check): NODE, WASM_DIR, SRCDIR, TEST_DIR.

SRCDIR=$(cd "${SRCDIR:-$(dirname "$0")/../../..}" && pwd)
TEST_DIR=${TEST_DIR:+$TEST_DIR/83-bloc}
. "$SRCDIR/tests/wasm/lib/common.sh"
lane=bloc
started=$(date +%s)
SMOKE=$SRCDIR/packaging/bloc/smoke.st
image=$WASM_DIR/image/stock-bloc

if ! ls "$image"/*.image >/dev/null 2>&1; then
    skip "the build has no Bloc image (WASM_EXTRA_IMAGES has no bloc=<zip>)"
fi
for vm in node web; do
    if ! grep -q '^extern const PharoFFILibrary pharoFFILibrary_harfbuzz;$' "$WASM_DIR/cmake/wasm/ffi/ffiRegistry-$vm.c" 2>/dev/null; then
	skip "the $vm VM has no HarfBuzz in its registry (WASM_PANGO is off)"
    fi
done
for f in "$WASM_DIR/node/pharo" "$WASM_DIR/web/manifest.json" "$SMOKE"; do
    if ! test -f "$f"; then
	echo "FAIL 83-bloc: no $f"
	exit 1
    fi
done
echo "83-bloc: $WASM_DIR/node/pharo, $(ls "$image"/*.image | head -n 1)"

# registry
checks=$((checks + 1))
missing=
for vm in node web; do
    for library in cairo freetype harfbuzz; do
	grep -q "^extern const PharoFFILibrary pharoFFILibrary_$library;\$" "$WASM_DIR/cmake/wasm/ffi/ffiRegistry-$vm.c" \
	    || missing="$missing $library($vm)"
    done
done
# (the table of a library, ffi_<library>.c, is the same for both VMs)
for entry in cairo:cairo_push_group freetype:FT_Get_Sfnt_Table harfbuzz:hb_ft_font_create_referenced; do
    grep -q "{\"${entry#*:}\"," "$WASM_DIR/cmake/wasm/ffi/ffi_${entry%%:*}.c" 2>/dev/null \
	|| missing="$missing ${entry#*:}"
done
for f in libcairo.so.2 libfreetype.so.6 libharfbuzz.so.0; do
    test -f "$WASM_DIR/node/$f" || missing="$missing node/$f"
    grep -q "\"$f\"" "$WASM_DIR/web/manifest.json" || missing="$missing manifest.libraries:$f"
done
if test -n "$missing"; then
    fail registry "missing:$missing"
else
    ok registry "cairo freetype harfbuzz, and the placeholders"
fi

# smoke
fresh smoke "$image"
smoke() { limit 1200 "$WASM_DIR/node/pharo" --headless Pharo.image --no-default-preferences st --no-source "$SMOKE"; }
if run smoke smoke; then
    summary=$(grep '^Bloc smoke classes ' smoke.out | tail -n 1)
    if test "$status" -ne 0; then
	fail smoke "exit status $status: $summary $(grep '^Bloc smoke problem' smoke.out | head -n 3 | tr '\n' ' ')"
    elif ! grep -q 'ok$' <<EOF
$summary
EOF
    then
	fail smoke "no summary ending in ok: '$summary'"
    elif ! grep -q '^Bloc smoke: .*PharoBlocMorphicForm needed: true ' smoke.out; then
	fail smoke "PharoBlocMorphicForm not needed: $(grep 'needed:' smoke.out)"
    elif grep -Eq 'FFI callout (failed|adapted|trapped)' smoke.err; then
	fail smoke "FFI callouts on stderr: $(grep -E 'FFI callout' smoke.err | sort -u | head -n 3 | tr '\n' ' ')"
    else
	ok smoke "$summary"
    fi
fi

finish "83-bloc: $(( $(date +%s) - started )) s"
