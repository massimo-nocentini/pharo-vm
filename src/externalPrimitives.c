/* sqExternalPrimitives.c -- Support functions for loading external primitives.
 *
 *   Copyright (C) 2016 by Ronie Salgado
 *   All rights reserved.
 *
 *   This file is part of Squeak.
 *
 *   Permission is hereby granted, free of charge, to any person obtaining a
 *   copy of this software and associated documentation files (the "Software"),
 *   to deal in the Software without restriction, including without limitation
 *   the rights to use, copy, modify, merge, publish, distribute, sublicense,
 *   and/or sell copies of the Software, and to permit persons to whom the
 *   Software is furnished to do so, subject to the following conditions:
 *
 *   The above copyright notice and this permission notice shall be included in
 *   all copies or substantial portions of the Software.
 *
 *   THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 *   IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 *   FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 *   AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 *   LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
 *   FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
 *   DEALINGS IN THE SOFTWARE.
 *
 * Author: roniesalg@gmail.com
 */

#ifdef WIN64
#include <windows.h>
#endif

#include "pharovm/pharo.h"

int sqVMOptionTraceModuleLoading = 0;

void *loadModuleHandle(const char *fileName);
sqInt freeModuleHandle(void *module);
void *getModuleSymbol(void *module, const char *symbol);

const char *moduleNamePatterns[] = {
    "%s%s",
#if defined(_WIN32)
    "%s%s.dll",
    "%slib%s.dll",
#elif defined(__APPLE__)
    "%s%s",
    "%s%s.dylib",
    "%slib%s.dylib",
#else
    "%s%s.so",
    "%slib%s.so",
#endif
    NULL
};

char moduleNameBuffer[FILENAME_MAX];

void * tryToLoadModuleInPath(char *path, const char *moduleName)
{
    void *moduleHandle;
    int i;

    for(i = 0; moduleNamePatterns[i] != NULL; i++)
    {
        snprintf(moduleNameBuffer, FILENAME_MAX, moduleNamePatterns[i], path, moduleName);
        moduleNameBuffer[FILENAME_MAX - 1] = 0;
        moduleHandle = loadModuleHandle(moduleNameBuffer);
        if(moduleHandle){
        	return moduleHandle;
        }
    }

    return 0;
}

void *
ioLoadModule(char *pluginName)
{
    void *moduleHandle;
    char** paths = getPluginPaths();
    int i;

    for(i = 0; paths[i] != NULL; i++){
    	moduleHandle = tryToLoadModuleInPath(paths[i], pluginName);
    	if(moduleHandle)
    		return moduleHandle;
    }

    moduleHandle = tryToLoadModuleInPath((char*) "", pluginName);
    if(moduleHandle)
        return moduleHandle;

    char **currentPath = getSystemSearchPaths();
    for(; *currentPath; ++currentPath)
    {
        moduleHandle = tryToLoadModuleInPath(*currentPath, pluginName);
        if(moduleHandle)
            return moduleHandle;
    }

    logDebug("Failed to load module: %s\n", pluginName);

    return 0;
}

sqInt
ioFreeModule(void *moduleHandle)
{
    return freeModuleHandle(moduleHandle);
}

void *
ioFindExternalFunctionInAccessorDepthInto(char *lookupName, void *moduleHandle,
											sqInt *accessorDepthPtr)
{
    void *function;

    if (!*lookupName) /* avoid errors in dlsym from eitherPlugin: code. */
      return 0;

#if defined(__EMSCRIPTEN__)
    /* Every plugin is built in, and its primitives are found in the tables
     * of src/common/sqNamedPrims.c: the functions of a library of the FFI
     * have signatures of their own, and the interpreter must not call them
     * as primitives.
     */
    if (accessorDepthPtr)
      return 0;
#endif

    function = getModuleSymbol(moduleHandle, lookupName);

    if (function && accessorDepthPtr)
    {
        char buf[256];
        signed char *accessorDepthVarPtr;

#ifdef _WIN32
        /*
        * Unsafe version of deprecated strcpy for compatibility
        * - does not check error code
        */
        strcpy_s(buf, 256, lookupName);
#else
        strcpy(buf, lookupName);
#endif
    	snprintf(buf+strlen(buf), sizeof(buf) - strlen(buf), "AccessorDepth");
    	accessorDepthVarPtr = (signed char *)getModuleSymbol(moduleHandle, buf);
    	/* The Slang machinery assumes accessor depth defaults to -1, which
    	 * means "no accessor depth".  It saves space not outputting -1 depths.
    	 */
    	*accessorDepthPtr = accessorDepthVarPtr
    							? *accessorDepthVarPtr
    							: -1;

    	//If the primitive does not have accessor depth we generate a warning.
    	if(accessorDepthVarPtr == NULL)
    		logDebug("Missing Accessor Depth: %s", lookupName);
    }

    return function;
}

#if defined(_WIN32)

void *
loadModuleHandle(const char *fileName)
{
    WCHAR convertedPath[MAX_PATH + 1];
    CHAR copiedFileName[MAX_PATH + 1];
    int len;

    len = strlen(fileName);
    memcpy(copiedFileName, fileName, len);
    copiedFileName[len] = 0;

    MultiByteToWideChar(CP_UTF8, 0, copiedFileName, -1, convertedPath, MAX_PATH + 1);

    logDebug("Try loading  %s\n", copiedFileName);

    HMODULE m = LoadLibraryW(convertedPath);

   	return m;
}

sqInt
freeModuleHandle(void *module)
{
    return FreeLibrary((HMODULE)module) ? 1 : 0;
}

void *
getModuleSymbol(void *module, const char *symbol)
{
	FARPROC address = GetProcAddress((HMODULE)(module ? module : GetModuleHandle(NULL)), symbol);

	if(address == NULL){

		DWORD errorCode = GetLastError();
		char* errorMessage = formatMessageFromErrorCode(errorCode);

		logDebug("Looking up symbol %s: %s", symbol, errorMessage);
		free(errorMessage);	  
	}

	if(address == NULL && module == NULL){
	  logDebug("Retrying in VM DLL");
	  void * vmModule;

	  vmModule = GetModuleHandleW(L"PharoVMCore.dll");
	  return getModuleSymbol(vmModule, symbol);
	}

    return (void*) address;
}

#elif defined(__EMSCRIPTEN__)

/* Emscripten defines __unix__ too, but there is no dlopen.  The plugins are
 * built in (src/common/sqNamedPrims.c looks them up in its tables), and the
 * libraries of the FFI are linked in and listed in the registry of
 * include/pharovm/emscripten/ffiRegistry.h: a module handle is a library of
 * the registry.  Without the FFI there are no modules: the image then finds
 * no primitiveLoadSymbolFromModule, and uses its NullFFIBackend.
 */

#if FEATURE_FFI

#include <ctype.h>
#include <stdlib.h>
#include <string.h>
#include "pharovm/emscripten/ffiRegistry.h"

typedef struct {
	char *pluginName;
	char *primitiveName;
	void *primitiveAddress;
} sqExport;

extern sqExport *pluginExports[];

/* The name of fileName that the registry knows: its base name, without a
 * leading "lib", nor anything from its first '.' on (.so.6, .dylib, .dll),
 * nor a version after a '-' (libSDL2-2.0.so.0 is SDL2, libc.so.6 is c,
 * libgit2.so.1.4.4 is git2).
 */
static void
reduceLibraryName(const char *fileName, char *reduced, size_t size)
{
	const char *base = strrchr(fileName, '/');
	size_t length = 0;

	base = base ? base + 1 : fileName;
	if (strncmp(base, "lib", 3) == 0 && base[3] && base[3] != '.')
		base += 3;
	while (base[length] && base[length] != '.' && length + 1 < size)
		length += 1;
	memcpy(reduced, base, length);
	reduced[length] = 0;
	for (char *dash = strchr(reduced, '-'); dash; dash = strchr(dash + 1, '-'))
		if (isdigit((unsigned char)dash[1])) {
			*dash = 0;
			break;
		}
}

/* Whether PHARO_WASM_FFI_HIDE, a comma-separated list of reduced names that
 * the environment gives (read at the first load), holds name: the negative
 * controls of the tests make a library of the registry missing so.
 */
static int
isHiddenLibrary(const char *name)
{
	static const char *hidden;
	static int read = 0;
	size_t length = strlen(name);

	if (!read) {
		hidden = getenv("PHARO_WASM_FFI_HIDE");
		hidden = hidden ? strdup(hidden) : NULL;
		read = 1;
	}
	for (const char *item = hidden; item && *item; ) {
		const char *end = strchr(item, ',');
		size_t itemLength = end ? (size_t)(end - item) : strlen(item);

		if (itemLength == length && strncmp(item, name, length) == 0)
			return 1;
		item = end ? end + 1 : item + itemLength;
	}
	return 0;
}

/* The library of the registry that the reduced name of fileName, or one of
 * its aliases, names, unless PHARO_WASM_FFI_HIDE holds that name or the
 * library's own.  The first time a library is answered, its onLoad function
 * (if it has one) is called.
 */
void *
loadModuleHandle(const char *fileName)
{
	static unsigned char *loaded;	/* one flag for each library */
	char reduced[FILENAME_MAX];

	reduceLibraryName(fileName, reduced, sizeof(reduced));
	if (!reduced[0])
		return NULL;
	for (int i = 0; pharoFFILibraries[i]; i++) {
		const PharoFFILibrary *library = pharoFFILibraries[i];
		int found = strcmp(library->name, reduced) == 0;

		for (int j = 0; !found && library->aliases && library->aliases[j]; j++)
			found = strcmp(library->aliases[j], reduced) == 0;
		if (!found)
			continue;
		if (isHiddenLibrary(reduced) || isHiddenLibrary(library->name)) {
			logTrace("Library %s (%s) hidden by PHARO_WASM_FFI_HIDE\n", fileName, library->name);
			return NULL;
		}
		if (library->onLoad) {
			if (!loaded) {
				int count = 0;

				while (pharoFFILibraries[count])
					count += 1;
				if (!(loaded = calloc(count, 1))) {
					logError("No memory to load the library %s\n", library->name);
					return NULL;
				}
			}
			if (!loaded[i]) {
				loaded[i] = 1;
				library->onLoad();
			}
		}
		return (void *)library;
	}
	logTrace("No library %s (%s) in the FFI registry\n", fileName, reduced);
	return NULL;
}

static int
compareSymbol(const void *name, const void *symbol)
{
	return strcmp((const char *)name, ((const PharoFFISymbol *)symbol)->name);
}

/* A symbol of a library of the registry (its rows are sorted by strcmp);
 * with no module, a function of the VM's own module-less tables (vm_exports,
 * vmsupport_exports...), as dlsym(RTLD_DEFAULT) would find it natively:
 * TFFIBackend>>isAvailable looks up primitiveLoadSymbolFromModule so.  (The
 * names of those tables carry the accessor depth after their NUL.)
 */
void *
getModuleSymbol(void *module, const char *symbol)
{
	if (module) {
		const PharoFFILibrary *library = module;
		const PharoFFISymbol *found = bsearch(symbol, library->symbols, library->count,
			sizeof(PharoFFISymbol), compareSymbol);

		return found ? found->address : NULL;
	}
	for (int list = 0; pluginExports[list]; list++)
		for (sqExport *row = pluginExports[list]; row->pluginName || row->primitiveName; row++)
			if ((!row->pluginName || !row->pluginName[0])
			 && row->primitiveName && strcmp(row->primitiveName, symbol) == 0)
				return row->primitiveAddress;
	return NULL;
}

#else /* FEATURE_FFI */

void *
loadModuleHandle(const char *fileName)
{
	return NULL;
}

void *
getModuleSymbol(void *module, const char *symbol)
{
	return NULL;
}

#endif /* FEATURE_FFI */

sqInt
freeModuleHandle(void *module)
{
	return 0;
}

#elif defined(__linux__) || defined(__unix__) || defined(__APPLE__)

#include <dlfcn.h>

void *
loadModuleHandle(const char *fileName)
{
    int flags = RTLD_NOW | RTLD_GLOBAL;
#ifdef RTLD_DEEPBIND
    flags |= RTLD_DEEPBIND; /* Prefer local symbols in the shared object vs external symbols. */
#endif

    logTrace("Try loading  %s\n", fileName);
    return dlopen(fileName, flags);
}

sqInt
freeModuleHandle(void *module){
    return dlclose(module) == 0 ? 0 : 1;
}

void *
getModuleSymbol(void *module, const char *symbol)
{
    return dlsym(module ? module: dlopen(NULL,0), symbol);
}

#else

void *
loadModuleHandle(const char *fileName)
{
    return 0;
}

sqInt
freeModuleHandle(void *module)
{
	return 1;
}

static void *
getModuleSymbol(void *module, const char *symbol)
{
    return 0;
}

void *
getModuleSymbol(void *module, const char *symbol)
{
    return dlsym(module, symbol);
}

#endif
