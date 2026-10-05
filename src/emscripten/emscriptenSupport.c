/* emscriptenSupport.c -- support of the Emscripten (WebAssembly) VM
 *
 *  - vmsupport_exports: the module-less primitives of the VM's own support
 *    code, which the builtin tables list next to vm_exports;
 *  - with the FFI, the guard around libffi's ffi_call, through which the
 *    interpreter makes every callout, adapted (ffiAdapt.c) when its
 *    declaration differs from the function in width;
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
#else /* FEATURE_FFI */
/* vmsupport_exports is generated from the primitives of src/ffi
 * (cmake/emscripten/genSupportTable.cmake).
 */
#include <ffi.h>

/* libffi 3.8.0 uses these from the JavaScript of its ffi_call and closures
 * without declaring them (later versions do).
 */
EM_JS_DEPS(pharoFFI, "$stackSave,$stackAlloc,$stackRestore,$getWasmTableEntry");

/* Call call(cif, fn, rvalue, avalue), which is ffi_call, from JavaScript, and
 * answer 0, or 1 when it threw.  libffi's ffi_call is JavaScript: it calls
 * the function from JavaScript, converting its arguments and its result to
 * and from the types of the function's own signature.  So when the image's
 * declaration does not match the function, there is no trap of
 * call_indirect but a TypeError (a Number where the function takes an int64,
 * say), and the JavaScript exception would unwind the whole VM.  A TypeError
 * of the result's conversion is raised once the function has run: its side
 * effects stand.  A RuntimeError is a trap of the function itself (out of
 * bounds, unreachable, an indirect call of the wrong type in a callback), and
 * any other error, such as the exhaustion of the stack, is reported as one.
 * The exceptions of the runtime go on: longjmp (a WebAssembly.Exception, or
 * an EmscriptenSjLj with WASM_SJLJ=emscripten), exit() (ExitStatus) and
 * 'unwind'.  The stack pointer is restored, since ffi_call had moved it.
 */
EM_JS_DEPS(pharoFFIGuard, "$ExitStatus");
EM_JS(int, emscriptenGuardedFFICall, (void *call, void *cif, void *fn, void *rvalue, void *avalue), {
	var sp = stackSave();
	try {
		getWasmTableEntry(Number(call))(cif, fn, rvalue, avalue);
		return 0;
	} catch (e) {
		if ((typeof WebAssembly.Exception != "undefined" && e instanceof WebAssembly.Exception)
		 || e instanceof ExitStatus || e == "unwind"
		 || (typeof EmscriptenSjLj != "undefined" && e instanceof EmscriptenSjLj))
			throw e;
		stackRestore(sp);
		var what = (e && e.name ? e.name + ": " : "") + (e && e.message !== undefined ? e.message : e);
		if (e instanceof TypeError)
			err("FFI callout failed, its declaration does not match function " + Number(fn) + ": " + what);
		else
			err("FFI callout trapped in function " + Number(fn) + ": " + what);
		return 1;
	}
});

/* The guarded ffi_call, which emscriptenAdaptedFFICall calls (ffiAdapt.c) */
static int
guardedFFICall(ffi_cif *cif, void (*fn)(void), void *rvalue, void **avalue)
{
	return emscriptenGuardedFFICall((void *)ffi_call, cif, (void *)fn, rvalue, avalue);
}

int emscriptenAdaptedFFICall(ffi_cif *cif, void (*fn)(void), void *rvalue, void **avalue,
	int (*call)(ffi_cif *cif, void (*fn)(void), void *rvalue, void **avalue));

/* Every callout of the interpreter (-Dffi_call=emscriptenFFICall on its
 * translation unit, cmake/Emscripten.cmake), adapted by
 * emscriptenAdaptedFFICall to the signature of the function in the registry
 * when its declaration has other widths.  A callout that threw, or whose
 * adapted call of a variadic function cannot be prepared, fails with
 * PrimErrFFIException, which doPrimitiveSameThreadCallout checks before it
 * pushes the result.  But an exception that passed through a callback
 * (sameThreadCallbackEnter, src/ffi/sameThread/sameThread.c, which then did
 * not restore the depth of callbacks) has unwound the interpreter of that
 * callback too: the VM cannot go on.
 */
void
emscriptenFFICall(ffi_cif *cif, void (*fn)(void), void *rvalue, void **avalue)
{
	int callbackDepth = emscriptenCallbackDepth;

	if (!emscriptenAdaptedFFICall(cif, fn, rvalue, avalue, guardedFFICall))
		return;
	if (emscriptenCallbackDepth != callbackDepth)
		error("an FFI callout failed inside a callback, which it unwound");
	primitiveFailFor(PrimErrFFIException);
}
#endif /* FEATURE_FFI */


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
