/* textSupport.c -- the fonts of Pango and Poppler in the Emscripten VM
 *
 * With WASM_PANGO, the VMs link GLib, fontconfig, Pango and (WASM_POPPLER)
 * Poppler, which find their fonts through fontconfig
 * (cmake/emscripten/deps/pango.cmake).  The fontconfig file of the VM, with
 * the DejaVu fonts next to it, is
 *
 *   - in the web VM, /usr/share/fonts/pharo/fonts.conf, embedded in its file
 *     system (--embed-file);
 *   - in the node VM, fonts/fonts.conf next to pharo.js, which the launcher
 *     gives in FONTCONFIG_FILE (packaging/emscripten/node/launch.sh.in).
 *
 * pharoWasmTextInit, the ON_LOAD function of the pango, pangocairo and
 * poppler-glib libraries of the registry (cmake/emscripten/ffiLibraries.cmake),
 * runs before the image first calls one of them, and so before fontconfig
 * reads its configuration: it sets FONTCONFIG_FILE to the web VM's file,
 * unless the environment gives one, so that fontconfig does not look for the
 * configuration of a system (/etc/fonts/fonts.conf) there is none of.
 */

#include <stdlib.h>
#include <unistd.h>

#define PHARO_WASM_FONTS_CONF "/usr/share/fonts/pharo/fonts.conf"

void
pharoWasmTextInit(void)
{
	static int done = 0;

	if (done)
		return;
	done = 1;
	if (getenv("FONTCONFIG_FILE") == NULL && access(PHARO_WASM_FONTS_CONF, R_OK) == 0)
		setenv("FONTCONFIG_FILE", PHARO_WASM_FONTS_CONF, 1);
}
