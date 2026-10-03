#ifdef PHARO_BUILTIN_PLUGINS_HEADER
/* A VM that links its plugins in generates pluginExports and pluginPrimitives */
#include PHARO_BUILTIN_PLUGINS_HEADER
#else
extern sqExport vm_exports[];
extern sqExport os_exports[];

sqExport *pluginExports[] = {
	vm_exports,
	os_exports,
//	SecurityPlugin_exports,
	NULL
};
#endif
