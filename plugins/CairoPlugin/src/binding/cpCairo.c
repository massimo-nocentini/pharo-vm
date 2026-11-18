/* cpCairo.c -- the binding layer of the CairoPlugin: fills the function
 * table of cpCairo.h, natively from a symbol loader (the plugin's calls the
 * VM's module loader with the library path of the image's FFI), and under
 * Emscripten (CP_STATIC_CAIRO) statically from pharo_cairo, where cairo.h is
 * included and every assignment below is type-checked against it.
 *
 * No VM header and no libc header (tests/bindtest.c links this file without a
 * VM; the targets lane compiles it -ffreestanding for every shipped ABI). */

#include "cpCairo.h"

#define CP_X_NAME(ret, name, params, flags) "cairo_" #name,
CP_HIDDEN const char *const cp_cairo_function_names[CP_FN_COUNT] = {
	CP_CAIRO_FUNCTIONS(CP_X_NAME)
};
#undef CP_X_NAME

#define CP_X_FLAGS(ret, name, params, flags) flags,
CP_HIDDEN const unsigned char cp_cairo_function_flags[CP_FN_COUNT] = {
	CP_CAIRO_FUNCTIONS(CP_X_FLAGS)
};
#undef CP_X_FLAGS

CP_HIDDEN int
cp_cairo_bind(cp_symbol_loader load, void *context, cp_cairo_api *api, const char **missing)
{
	cp_cairo_api local;
	void *address;

	if (!load || !api) {
		return CP_BIND_BAD_ARGUMENT;
	}
	/* one lookup per function, in table order; a missing required one
	   stops the bind before *api is touched.  (A data pointer converted to
	   a function pointer: what dlsym answers, as POSIX requires.) */
#define CP_X_LOAD(ret, name, params, flags) \
	address = load("cairo_" #name, context); \
	if (!address && (flags) != CP_OPT) { \
		if (missing) { \
			*missing = "cairo_" #name; \
		} \
		return CP_BIND_MISSING; \
	} \
	local.name = (cp_fn_##name)address;
	CP_CAIRO_FUNCTIONS(CP_X_LOAD)
#undef CP_X_LOAD
	*api = local;
	return CP_BIND_OK;
}

#ifdef CP_STATIC_CAIRO
CP_HIDDEN int
cp_cairo_bind_static(cp_cairo_api *api)
{
	cp_cairo_api local;

#define CP_X_STATIC(ret, name, params, flags) local.name = cairo_##name;
	CP_CAIRO_FUNCTIONS(CP_X_STATIC)
#undef CP_X_STATIC
	*api = local;
	return CP_BIND_OK;
}
#endif
