/* ffiRegistry.h -- the libraries of the FFI of the Emscripten VM
 *
 * There is no dlopen: the libraries the image calls through the FFI are
 * linked into the VM, and the registry lists, for each one, its symbols with
 * their addresses, taken in a translation unit that includes the library's
 * own headers (cmake/emscripten/ffiRegistry.cmake generates it).  A
 * function's address is its index in the table of the module, and ffi_call
 * calls it with the types of its own signature: a declaration with another
 * prototype would give the address of a stub of wasm-ld that traps.
 *
 * src/externalPrimitives.c finds a library by the name the image gives,
 * reduced to its base name without "lib", directories and suffixes:
 * libc.so.6 is "c", /pharo/libSDL2-2.0.so.0 is "SDL2".  The node VM and the
 * web VM have a registry each, since a library can be in the node VM only
 * (the FFI test library).
 */

#ifndef PHARO_FFI_REGISTRY_H
#define PHARO_FFI_REGISTRY_H

#include <stddef.h>

typedef struct {
	const char *name;
	void *address;
	/* The WebAssembly signature of the function, as its declaration in the
	 * headers lowers it, for a callout whose declaration differs from it
	 * in width (src/emscripten/ffiAdapt.c): '<result><parameters>' in
	 * v, i, j, f and d, then '.<fixed arguments>' for a variadic function
	 * (cmake/emscripten/ffiSignatures.mjs).  NULL when it is not known:
	 * for data, and for the functions of a library whose table defines
	 * them (its SOURCES, the FFI test library). */
	const char *signature;
} PharoFFISymbol;

typedef struct {
	const char *name;              /* the reduced name */
	const char *const *aliases;    /* other reduced names, NULL-terminated */
	const PharoFFISymbol *symbols; /* sorted by name (strcmp) */
	size_t count;
	/* Called once, the first time the library is loaded, or NULL */
	void (*onLoad)(void);
} PharoFFILibrary;

/* NULL-terminated */
extern const PharoFFILibrary *const pharoFFILibraries[];

#endif /* PHARO_FFI_REGISTRY_H */
