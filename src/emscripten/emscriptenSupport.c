/* emscriptenSupport.c -- support of the Emscripten (WebAssembly) VM
 *
 *  - vmsupport_exports: the module-less primitives of the VM's own support
 *    code, which the builtin tables list next to vm_exports;
 *  - the notification of the host when the image has been saved;
 *  - listings of the builtin primitive tables and of what the lookups of
 *    src/common/sqNamedPrims.c answer for them, for tests/wasm/prim-audit.mjs.
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "pharovm/pharo.h"
#include "pharovm/imageAccess.h"

#include <emscripten.h>

#if !FEATURE_FFI
/* The FFI is not built, so the support primitives of src/ffi are missing and
 * the image falls back to its NullFFIBackend.  But the image initializes the
 * callbacks at every start, and says on stdout that it cannot: keep that
 * silent.
 */
static void
primitiveInitilizeCallbacks(void)
{
	pop(methodArgumentCount());
}

static char _m[] = "";
void *vmsupport_exports[][3] = {
	{(void*)_m, "primitiveInitilizeCallbacks\000\000", (void*)primitiveInitilizeCallbacks},
	{NULL, NULL, NULL}
};
#endif /* !FEATURE_FFI */


/* Tell the host, through Module.onPharoHost(kind, text), about something that
 * happened in the VM.  The host must not call back into the VM from there:
 * it queues the notification and handles it once the slice is over.
 */
EM_JS_DEPS(pharoHostNotify, "$UTF8ToString");
EM_JS(void, pharo_host_notify, (int kind, const char *text), {
	text = Number(text);
	if (Module['onPharoHost'])
		Module['onPharoHost'](kind, text ? UTF8ToString(text) : "");
});

/* The image files the VM has opened for writing, so that closing one is known
 * to have saved the image.  This catches every save, whether it was asked for
 * by the host, a menu or a script.
 */
#define MAX_WRITTEN_FILES 4
static struct { sqImageFile file; char *path; } writtenFiles[MAX_WRITTEN_FILES];
static FileAccessHandler *baseFileAccessHandler;
static FileAccessHandler notifyingFileAccessHandler;

static sqImageFile
notifyingImageFileOpen(const char *fileName, char *mode)
{
	sqImageFile file = baseFileAccessHandler->imageFileOpen(fileName, mode);

	if (file && strchr(mode, 'w'))
		for (int i = 0; i < MAX_WRITTEN_FILES; i++)
			if (!writtenFiles[i].file) {
				writtenFiles[i].file = file;
				writtenFiles[i].path = strdup(fileName);
				break;
			}
	return file;
}

static sqInt
notifyingImageFileClose(sqImageFile file)
{
	sqInt result = baseFileAccessHandler->imageFileClose(file);

	for (int i = 0; i < MAX_WRITTEN_FILES; i++)
		if (file && writtenFiles[i].file == file) {
			writtenFiles[i].file = NULL;
			if (writtenFiles[i].path)
				pharo_host_notify(HOST_IMAGE_SAVED, writtenFiles[i].path);
			free(writtenFiles[i].path);
			writtenFiles[i].path = NULL;
		}
	return result;
}

void
emscriptenInstallFileAccessHandler(void)
{
	baseFileAccessHandler = currentFileAccessHandler();
	notifyingFileAccessHandler = *baseFileAccessHandler;
	notifyingFileAccessHandler.imageFileOpen = notifyingImageFileOpen;
	notifyingFileAccessHandler.imageFileClose = notifyingImageFileClose;
	setFileAccessHandler(&notifyingFileAccessHandler);
}


/* The builtin plugin tables, as src/common/sqNamedPrims.c declares them. */
typedef struct {
	char *pluginName;
	char *primitiveName;
	void *primitiveAddress;
} sqExport;

extern sqExport *pluginExports[];
extern sqExport *pluginPrimitives[];

/* Print a line "<module> <name> <table index> <exports table index>" for every
 * row of pluginPrimitives, the tables the named-primitive lookup dispatches
 * through (module "-" for the VM's own module-less rows).  The last field is
 * the same row's function in pluginExports: the two differ exactly where the
 * row goes through a void trampoline.  Answer the number of rows.
 */
EMSCRIPTEN_KEEPALIVE int
vm_list_builtin_primitives(void)
{
	int rows = 0;

	for (int list = 0; pluginPrimitives[list]; list++)
		for (int i = 0; pluginPrimitives[list][i].pluginName || pluginPrimitives[list][i].primitiveName; i++) {
			sqExport *row = &pluginPrimitives[list][i];

			printf("%s %s %llu %llu\n",
				row->pluginName && row->pluginName[0] ? row->pluginName : "-",
				row->primitiveName,
				(unsigned long long)(uintptr_t)row->primitiveAddress,
				(unsigned long long)(uintptr_t)pluginExports[list][i].primitiveAddress);
			rows += 1;
		}
	fflush(stdout);
	return rows;
}

/* Print what the lookups of src/common/sqNamedPrims.c answer for the names
 * of each row of pluginExports, through their entry points: as the
 * interpreter looks up a named primitive (with an accessor depth) and as
 * ioLoadFunctionFrom looks up a function (without one), as a line
 * "lookup <module> <name> <primitive> <depth> <function>", module "-" for the
 * VM's own rows, the functions as table indices.  <depth> is the accessor
 * depth the lookup answers, which means something only for the rows whose
 * name carries one.  (A lookup loads the module of the row first, which
 * initializes its plugin.)  Answer the number of rows.
 */
EMSCRIPTEN_KEEPALIVE int
vm_list_builtin_lookups(void)
{
	int rows = 0;

	for (int list = 0; pluginExports[list]; list++)
		for (int i = 0; pluginExports[list][i].pluginName || pluginExports[list][i].primitiveName; i++) {
			char *module = pluginExports[list][i].pluginName ? pluginExports[list][i].pluginName : "";
			char *name = pluginExports[list][i].primitiveName;
			sqInt depth = 0;
			void *primitive = ioLoadExternalFunctionOfLengthFromModuleOfLengthAccessorDepthInto(
				oopForPointer(name), strlen(name), oopForPointer(module), strlen(module), &depth);
			void *function = ioLoadFunctionFrom(name, module);

			printf("lookup %s %s %llu %lld %llu\n",
				module[0] ? module : "-", name,
				(unsigned long long)(uintptr_t)primitive, (long long)depth,
				(unsigned long long)(uintptr_t)function);
			rows += 1;
		}
	fflush(stdout);
	return rows;
}
